//! Shared draft, review and background publication for the two frontends.
use crate::{
    edit_field,
    git_merge_resolution::{MergeAction, MergeDraft, MergeFile, MergePane},
    git_repository::{
        repository_actions, RepositoryAction, RepositoryDraft, RepositoryOutcome,
        RepositoryResolution, RepositoryReview, StashTarget,
    },
    git_revision::{ComparisonOrigin, PairKind, RevisionComparison},
    present_field, FieldEdit, FieldPresentation, Wakeup, Workbench,
};
use jc_git::branch_switch::{apply_branch_switch, prepare_branch_switch, BranchSwitchPlan};
pub use jc_git::commit_write::AmendTarget;
pub use jc_git::commit_write::MESSAGE_BYTES;
use jc_git::commit_write::{apply_commit, prepare_amend, prepare_commit, CommitPlan};
use jc_git::local_merge::{
    apply_merge, apply_merge_abort, prepare_commit_mainline, prepare_commit_operation,
    prepare_interactive_rebase, prepare_merge, prepare_merge_abort, prepare_rebase,
    prepare_rebase_todo, prepare_reset, prepare_resolved_merge, MergeAbortPlan, MergeKind,
    MergePlan, MergeReview, RebaseStep, RebaseStepState,
};
pub use jc_git::local_merge::{CommitOperation, RebaseAction, ResetMode};
use jc_git::local_pair::SourceIdentity;
pub use jc_git::ref_write::RefKind;
use jc_git::ref_write::{
    apply_ref, apply_ref_change, prepare_ref, prepare_ref_change, RefChangePlan, RefPlan,
    REF_NAME_BYTES,
};
use jc_git::remote::{OperationCredential, SecretString};
use jc_git::restore::{apply_restore, prepare_restore, RestorePlan};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const LIMITS: jc_git::Limits = jc_git::Limits {
    max_blob: 2 * 1024 * 1024,
    max_entries: 8192,
    max_path: 4096,
    max_bytes: 4 * 1024 * 1024,
    timeout: Duration::from_secs(10),
};

enum Completion {
    Review(Result<WritePlan, jc_git::Error>),
    Applied(Result<String, jc_git::Error>),
    PullReview(Box<MergePlan>),
    Panic,
}
enum WritePlan {
    Commit(Box<CommitPlan>),
    Reference(Box<RefPlan>),
    ReferenceChange(Box<RefChangePlan>),
    BranchSwitch(Box<BranchSwitchPlan>),
    Merge(Box<MergePlan>),
    MergeAbort(Box<MergeAbortPlan>),
    Restore(Box<RestorePlan>),
    Repository(Box<RepositoryReview>),
}

impl WritePlan {
    fn merge(&self) -> Option<&MergePlan> {
        match self {
            Self::Merge(plan) => Some(plan),
            Self::Repository(plan) => plan.merge_plan(),
            _ => None,
        }
    }
}

enum ResolutionReview {
    Merge(Box<MergeReview>),
    Repository(Box<RepositoryResolution>),
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReferenceTarget {
    pub kind: RefKind,
    pub name: String,
    pub oid: String,
}

#[derive(Clone)]
pub enum WriteDraft {
    Commit,
    CreateReference(RefKind, String),
    ChangeReference(ReferenceTarget, bool),
    SwitchBranch(ReferenceTarget),
    MergeBranch(ReferenceTarget),
    RebaseBranch(ReferenceTarget, bool),
    CommitOperation(CommitOperation, String),
    Reset(ResetMode, String),
    AbortMerge,
    Restore(Arc<RevisionComparison>),
    RepositoryMenu(Option<StashTarget>),
    Repository(RepositoryDraft),
}

impl WriteDraft {
    fn is_reference(&self) -> bool {
        matches!(self, Self::CreateReference(..) | Self::ChangeReference(..))
    }

    fn has_input(&self) -> bool {
        if let Self::Repository(draft) = self {
            return draft.action.prompt().is_some();
        }
        !matches!(
            self,
            Self::ChangeReference(_, false)
                | Self::SwitchBranch(_)
                | Self::MergeBranch(_)
                | Self::RebaseBranch(..)
                | Self::CommitOperation(..)
                | Self::Reset(..)
                | Self::AbortMerge
                | Self::Restore(_)
                | Self::RepositoryMenu(_)
        )
    }

    fn publishes_integration(&self) -> bool {
        matches!(
            self,
            Self::MergeBranch(_) | Self::RebaseBranch(..) | Self::CommitOperation(..)
        )
    }

    fn can_stop_publication(&self) -> bool {
        self.publishes_integration() || matches!(self, Self::Reset(..))
    }

    fn refreshes_graph(&self) -> bool {
        self.is_reference()
            || self.publishes_integration()
            || matches!(
                self,
                Self::SwitchBranch(_) | Self::AbortMerge | Self::Reset(..) | Self::Repository(_)
            )
    }

    fn refreshes_files(&self) -> bool {
        if let Self::Repository(draft) = self {
            return draft.action.changes_files();
        }
        self.publishes_integration()
            || matches!(
                self,
                Self::Restore(_)
                    | Self::SwitchBranch(_)
                    | Self::AbortMerge
                    | Self::Reset(ResetMode::Hard, _)
            )
    }
}

pub fn restore_label(comparison: &RevisionComparison) -> Option<&'static str> {
    let ComparisonOrigin::Local(local) = &comparison.origin else {
        return None;
    };
    if !cfg!(windows)
        || local.kind != PairKind::Worktree
        || !matches!(local.before, SourceIdentity::Present { mode: 0o100644, .. })
    {
        return None;
    }
    if local.after.is_absent() {
        Some("Recuperar archivo")
    } else if matches!(local.after, SourceIdentity::Present { mode: 0o100644, .. })
        && comparison.can_export_patch()
    {
        Some("Restaurar archivo")
    } else {
        None
    }
}

struct Job {
    result: Arc<Mutex<Option<Completion>>>,
    thread: JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    applying: bool,
}

pub struct CommitView {
    root: PathBuf,
    pub replaces: Option<AmendTarget>,
    pub draft: WriteDraft,
    pub message: String,
    pub cursor: usize,
    pub first: usize,
    pub error: Option<String>,
    secret: Option<SecretEntry>,
    credentials: Option<Arc<OperationCredential>>,
    input_token: Arc<()>,
    review: Option<WritePlan>,
    merge_draft: MergeDraft,
    saved_merge_review: Option<Box<MergePlan>>,
    saved_mainline_review: Option<Box<MergePlan>>,
}

const SECRET_LIMIT: usize = 2048;

fn split_network_identity(input: &str, action: RepositoryAction) -> Option<(&str, &str)> {
    let delimiter = input
        .match_indices('|')
        .nth(usize::from(matches!(
            action,
            RepositoryAction::CloneWorktree
                | RepositoryAction::InitSubmodule
                | RepositoryAction::UpdateSubmodule
        )))?
        .0;
    Some((&input[..delimiter], &input[delimiter + 1..]))
}

struct SecretEntry {
    input: Vec<u8>,
    username: String,
    pin: Option<String>,
    private_key: Option<PathBuf>,
}

impl SecretEntry {
    fn new(username: String, pin: Option<String>, private_key: Option<PathBuf>) -> Self {
        Self {
            input: Vec::with_capacity(SECRET_LIMIT),
            username,
            pin,
            private_key,
        }
    }

    fn into_credentials(mut self) -> OperationCredential {
        OperationCredential {
            username: std::mem::take(&mut self.username),
            secret: SecretString::new(
                String::from_utf8(std::mem::take(&mut self.input))
                    .expect("La entrada de credencial siempre es UTF-8"),
            ),
            host_fingerprint_pin: self.pin.take(),
            private_key: self.private_key.take(),
        }
    }

    fn edit(&mut self, edit: FieldEdit<'_>) -> Result<(), &'static str> {
        match edit {
            FieldEdit::Insert(text) => {
                if text.len() > SECRET_LIMIT.saturating_sub(self.input.len())
                    || text.chars().any(char::is_control)
                {
                    return Err("Credencial fuera del límite o con caracteres de control");
                }
                self.input.extend_from_slice(text.as_bytes());
            }
            FieldEdit::Backspace => {
                if let Some((start, _)) = self.text().char_indices().next_back() {
                    self.truncate(start);
                }
            }
            FieldEdit::Clear => self.truncate(0),
            _ => {}
        }
        Ok(())
    }

    fn text(&self) -> &str {
        std::str::from_utf8(&self.input).expect("La entrada de credencial siempre es UTF-8")
    }

    fn truncate(&mut self, start: usize) {
        for byte in &mut self.input[start..] {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        self.input.truncate(start);
    }
}

impl Drop for SecretEntry {
    fn drop(&mut self) {
        self.truncate(0);
    }
}

impl CommitView {
    pub fn title(&self) -> &'static str {
        match &self.draft {
            WriteDraft::CreateReference(RefKind::Branch, _) => "Crear rama",
            WriteDraft::CreateReference(RefKind::Tag, _) => "Crear etiqueta",
            WriteDraft::ChangeReference(target, rename) => match (target.kind, rename) {
                (RefKind::Branch, true) => "Renombrar rama",
                (RefKind::Tag, true) => "Renombrar etiqueta",
                (RefKind::Branch, false) => "Eliminar rama",
                (RefKind::Tag, false) => "Eliminar etiqueta",
            },
            WriteDraft::SwitchBranch(_) => "Cambiar rama",
            WriteDraft::MergeBranch(_) => "Fusionar rama",
            WriteDraft::RebaseBranch(_, true) => "Rebase interactivo",
            WriteDraft::RebaseBranch(_, false) => "Rebase local",
            WriteDraft::CommitOperation(operation, _) => operation.title(),
            WriteDraft::Reset(..) => "Reset de commit",
            WriteDraft::AbortMerge => "Abortar operación Git",
            WriteDraft::Restore(comparison) => {
                restore_label(comparison).unwrap_or("Restaurar archivo")
            }
            WriteDraft::RepositoryMenu(_) => "Operaciones del repositorio",
            WriteDraft::Repository(draft) => draft.action.title(),
            WriteDraft::Commit if self.replaces.is_some() => "Modificar último commit",
            WriteDraft::Commit => "Crear commit",
        }
    }
    pub fn has_input(&self) -> bool {
        self.draft.has_input()
    }
    pub fn presenting_secret(&self) -> bool {
        self.secret.is_some()
    }

    pub fn present_input(&self, width: usize) -> FieldPresentation {
        if let Some(secret) = &self.secret {
            let masked = "*".repeat(secret.text().chars().count());
            return present_field(&masked, masked.len(), width, 1);
        }
        present_field(&self.message, self.cursor, width, 1)
    }

    pub fn repository_prompt(&self) -> Option<&'static str> {
        if self.presenting_secret() {
            return Some("Contraseña, token o frase de la clave");
        }
        match &self.draft {
            WriteDraft::Repository(draft) => draft.action.prompt(),
            _ => None,
        }
    }
    pub fn pending_hint(&self) -> &'static str {
        match &self.draft {
            WriteDraft::Commit => "Leyendo el índice preparado… · Esc cancela",
            WriteDraft::Restore(_) => "Comprobando el índice y el archivo… · Esc cancela",
            WriteDraft::SwitchBranch(_) => "Comprobando ambas ramas… · Esc cancela",
            WriteDraft::MergeBranch(_) => "Calculando la fusión local… · Esc cancela",
            WriteDraft::RebaseBranch(..) => "Revisando los commits del rebase… · Esc cancela",
            WriteDraft::CommitOperation(CommitOperation::CherryPick, _) => {
                "Calculando el cherry-pick… · Esc cancela"
            }
            WriteDraft::CommitOperation(CommitOperation::Revert, _) => {
                "Calculando la reversión… · Esc cancela"
            }
            WriteDraft::AbortMerge => "Revisando cambios de la operación… · Esc cancela",
            WriteDraft::Reset(..) => "Calculando el reset… · Esc cancela",
            WriteDraft::Repository(_) => "Revisando el repositorio… · Esc cancela",
            _ => "Comprobando referencia… · Esc cancela",
        }
    }
    pub fn review(&self) -> Option<&CommitPlan> {
        match &self.review {
            Some(WritePlan::Commit(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn repository_menu(&self) -> Option<impl Iterator<Item = RepositoryAction> + use<>> {
        let WriteDraft::RepositoryMenu(target) = &self.draft else {
            return None;
        };
        Some(repository_actions(target.is_some()))
    }
    pub fn repository_review(&self) -> Option<&RepositoryReview> {
        match &self.review {
            Some(WritePlan::Repository(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn reference_review(&self) -> Option<&RefPlan> {
        match &self.review {
            Some(WritePlan::Reference(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn reference_change_review(&self) -> Option<&RefChangePlan> {
        match &self.review {
            Some(WritePlan::ReferenceChange(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn restore_review(&self) -> Option<&RestorePlan> {
        match &self.review {
            Some(WritePlan::Restore(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn branch_switch_review(&self) -> Option<&BranchSwitchPlan> {
        match &self.review {
            Some(WritePlan::BranchSwitch(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn reviewing(&self) -> bool {
        self.review.is_some()
    }
    pub fn merge_review(&self) -> Option<&MergePlan> {
        self.review.as_ref()?.merge()
    }
    pub fn merge_conflicts(&self) -> Option<&MergePlan> {
        self.merge_review()
            .filter(|plan| plan.kind == MergeKind::Conflicts)
    }
    pub fn choosing_mainline(&self) -> Option<&MergePlan> {
        self.merge_review()
            .filter(|plan| plan.kind == MergeKind::ChooseMainline)
    }
    pub fn rebase_todo(&self) -> Option<&[jc_git::local_merge::RebaseTodo]> {
        self.merge_review()?.rebase_todo()
    }
    pub fn can_return_to_mainline(&self) -> bool {
        self.saved_mainline_review.is_some()
    }
    pub fn merge_abort_review(&self) -> Option<&MergeAbortPlan> {
        match &self.review {
            Some(WritePlan::MergeAbort(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn merge_file(&self) -> Option<&MergeFile> {
        self.merge_draft.file.as_ref()
    }
    pub fn merge_choice_saved(&self, path: &[u8]) -> bool {
        self.merge_draft.choices.contains_key(path)
    }
    pub fn all_merge_choices_saved(&self) -> bool {
        self.merge_conflicts()
            .is_some_and(|plan| self.merge_draft.choices.len() == plan.rows.len())
    }
    pub fn can_return_to_merge_choices(&self) -> bool {
        self.saved_merge_review.is_some()
    }
    fn input_limit(&self) -> usize {
        if self.draft.is_reference() {
            REF_NAME_BYTES
        } else {
            MESSAGE_BYTES - 1
        }
    }
}

pub struct CommitController {
    wake: Wakeup,
    view: Option<CommitView>,
    job: Option<Job>,
    notice: Option<(PathBuf, String)>,
}

impl CommitController {
    pub fn new(wake: Wakeup) -> Self {
        Self {
            wake,
            view: None,
            job: None,
            notice: None,
        }
    }

    pub fn view(&self) -> Option<&CommitView> {
        self.view.as_ref()
    }
    pub fn notice(&self, root: Option<&Path>) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(path, _)| Some(path.as_path()) == root)
            .map(|(_, notice)| notice.as_str())
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn applying(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.applying)
    }
    pub fn merge_publication_hint(&self) -> Option<&'static str> {
        let job = self.job.as_ref().filter(|job| job.applying)?;
        self.view.as_ref()?.draft.can_stop_publication().then(|| {
            if job.cancelled.load(Ordering::Acquire) {
                "Deteniendo la operación en un punto seguro…"
            } else {
                "Publicando cambios… · Esc detiene para revisar el aborto"
            }
        })
    }

    pub fn request_merge_stop(&mut self) -> bool {
        if self.merge_publication_hint().is_none() {
            return false;
        }
        self.job
            .as_ref()
            .unwrap()
            .cancelled
            .store(true, Ordering::Release);
        true
    }
    pub fn ready(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.result.lock().unwrap().is_some())
    }

    pub fn open(&mut self, root: PathBuf, replaces: Option<AmendTarget>) {
        self.open_write(root, replaces, WriteDraft::Commit);
    }
    pub fn open_reference(&mut self, root: PathBuf, kind: RefKind, target: String) {
        self.open_write(root, None, WriteDraft::CreateReference(kind, target));
    }
    pub fn open_reference_change(&mut self, root: PathBuf, target: ReferenceTarget, rename: bool) {
        self.open_write(root, None, WriteDraft::ChangeReference(target, rename));
    }
    pub fn open_branch_switch(&mut self, root: PathBuf, target: ReferenceTarget) {
        if cfg!(windows) && target.kind == RefKind::Branch {
            self.open_write(root, None, WriteDraft::SwitchBranch(target));
        }
    }
    pub fn open_merge(&mut self, root: PathBuf, target: ReferenceTarget) {
        if cfg!(windows) && target.kind == RefKind::Branch {
            self.open_write(root, None, WriteDraft::MergeBranch(target));
        }
    }

    pub fn open_rebase(&mut self, root: PathBuf, target: ReferenceTarget, interactive: bool) {
        if cfg!(windows) && target.kind == RefKind::Branch {
            self.open_write(root, None, WriteDraft::RebaseBranch(target, interactive));
        }
    }
    pub fn open_merge_abort(&mut self, root: PathBuf) {
        if cfg!(windows) {
            self.open_write(root, None, WriteDraft::AbortMerge);
        }
    }
    pub fn open_commit_operation(
        &mut self,
        root: PathBuf,
        commit: String,
        operation: CommitOperation,
    ) {
        if cfg!(windows) {
            self.open_write(root, None, WriteDraft::CommitOperation(operation, commit));
        }
    }
    pub fn open_reset(&mut self, root: PathBuf, commit: String) {
        if cfg!(windows) {
            self.open_write(root, None, WriteDraft::Reset(ResetMode::Soft, commit));
        }
    }

    pub fn select_reset_mode(&mut self, choice: ResetMode) {
        if self.busy() {
            return;
        }
        let Some(view) = self.view.as_mut().filter(|view| !view.reviewing()) else {
            return;
        };
        if let WriteDraft::Reset(mode, _) = &mut view.draft {
            *mode = choice;
            view.error = None;
        }
    }

    pub fn step_reset_mode(&mut self, delta: isize) {
        let Some(WriteDraft::Reset(current, _)) = self.view.as_ref().map(|view| &view.draft) else {
            return;
        };
        let modes = ResetMode::ALL;
        if let Some(index) = modes.iter().position(|mode| mode == current) {
            let next = (index as isize + delta).rem_euclid(modes.len() as isize) as usize;
            self.select_reset_mode(modes[next]);
        }
    }

    pub fn open_restore(&mut self, root: PathBuf, comparison: Arc<RevisionComparison>) {
        if restore_label(&comparison).is_some() {
            self.open_write(root, None, WriteDraft::Restore(comparison));
        }
    }
    pub fn open_repository_menu(&mut self, root: PathBuf, stash: Option<StashTarget>) {
        self.open_write(root, None, WriteDraft::RepositoryMenu(stash));
    }
    pub fn open_worktree_clone(&mut self, root: PathBuf) {
        self.open_write(
            root,
            None,
            WriteDraft::Repository(RepositoryDraft {
                action: RepositoryAction::CloneWorktree,
                stash: None,
            }),
        );
    }
    pub fn select_repository_action(&mut self, index: usize) {
        if self.busy() {
            return;
        }
        if let Some(view) = &mut self.view {
            if view
                .repository_menu()
                .is_some_and(|mut actions| actions.nth(index).is_some())
            {
                view.first = index;
            }
        }
    }
    fn open_write(&mut self, root: PathBuf, replaces: Option<AmendTarget>, draft: WriteDraft) {
        if self.job.is_none() && self.view.is_none() {
            self.view = Some(CommitView {
                root,
                replaces,
                draft,
                message: String::new(),
                cursor: 0,
                first: 0,
                error: None,
                secret: None,
                credentials: None,
                input_token: Arc::new(()),
                review: None,
                merge_draft: MergeDraft::default(),
                saved_merge_review: None,
                saved_mainline_review: None,
            });
        }
    }

    pub fn cancel(&mut self) {
        if self.applying() {
            return;
        }
        if let Some(job) = &self.job {
            job.cancelled.store(true, Ordering::Release);
        }
        self.view = None;
    }

    pub fn matches_root(&self, root: Option<&Path>) -> bool {
        self.view
            .as_ref()
            .is_none_or(|v| Some(v.root.as_path()) == root)
    }

    pub fn input_token(&self) -> Option<Arc<()>> {
        self.view
            .as_ref()
            .filter(|_| self.paste_budget().is_some())
            .map(|v| v.input_token.clone())
    }

    pub fn paste_budget(&self) -> Option<usize> {
        let view = self.view.as_ref().filter(|_| !self.busy())?;
        if let (Some(file), Some(plan)) = (view.merge_file(), view.merge_conflicts()) {
            return file.paste_budget(plan);
        }
        if let Some(secret) = &view.secret {
            return Some(SECRET_LIMIT.saturating_sub(secret.input.len()).max(1));
        }
        (view.has_input() && view.review.is_none())
            .then(|| view.input_limit().saturating_sub(view.message.len()).max(1))
    }

    pub fn edit(&mut self, edit: FieldEdit<'_>) {
        if self.busy() {
            return;
        }
        let Some(view) = self.view.as_mut() else {
            return;
        };
        if let (Some(plan), Some(file)) = (
            view.review.as_ref().and_then(WritePlan::merge),
            &mut view.merge_draft.file,
        ) {
            view.error = file.edit(plan, edit).err().map(str::to_owned);
            view.input_token = Arc::new(());
            return;
        }
        if !view.has_input() || view.review.is_some() {
            return;
        }
        if let Some(secret) = &mut view.secret {
            view.error = secret.edit(edit).err().map(str::to_owned);
            view.input_token = Arc::new(());
            return;
        }
        view.credentials = None;
        if let FieldEdit::Insert(text) = edit {
            if text.chars().any(char::is_control) {
                view.error = Some("El campo debe ocupar una sola línea".into());
                return;
            }
        }
        let limit = view.input_limit();
        view.error = edit_field(&mut view.message, &mut view.cursor, edit, limit)
            .err()
            .map(str::to_owned);
        view.input_token = Arc::new(());
    }

    pub fn merge_action(&mut self, action: MergeAction) {
        if self.busy() {
            return;
        }
        let Some(view) = &mut self.view else { return };
        let Some(plan) = view.review.as_ref().and_then(WritePlan::merge) else {
            return;
        };
        if plan.kind != MergeKind::Conflicts {
            return;
        }
        let file = &mut view.merge_draft.file;
        view.error = None;
        view.input_token = Arc::new(());
        match action {
            MergeAction::Open(row) if file.is_none() => {
                let Some(entry) = plan.rows.get(row) else {
                    return;
                };
                if plan
                    .conflict(&entry.path)
                    .is_some_and(|conflict| conflict.can_resolve())
                {
                    view.first = row;
                    *file = Some(MergeFile::open(
                        row,
                        view.merge_draft.choices.get(&entry.path),
                    ));
                } else {
                    view.error =
                        Some("La resolución interactiva requiere un archivo ordinario".into());
                }
            }
            MergeAction::Pane(pane) => {
                if let Some(file) = file {
                    file.set_pane(pane);
                }
            }
            MergeAction::NextPane => {
                if let Some(file) = file {
                    file.next_pane();
                }
            }
            MergeAction::UseVersion => {
                if let Some(file) = file {
                    file.use_version();
                }
            }
            MergeAction::Accept => {
                if file
                    .as_ref()
                    .is_some_and(|file| file.pane == MergePane::Result)
                {
                    let file = file.take().unwrap();
                    let path = file.path(plan).to_vec();
                    view.merge_draft.choices.insert(path, file.take_choice());
                }
            }
            MergeAction::Newline => {
                if let Some(file) = file {
                    view.error = file.newline(plan).err().map(str::to_owned);
                }
            }
            MergeAction::Open(_) => {}
        }
    }

    /// Return to the conflict list or the draft after reviewing the final tree.
    pub fn back_merge(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        let Some(view) = &mut self.view else {
            return false;
        };
        if view.merge_draft.file.take().is_some() {
            view.error = None;
            view.input_token = Arc::new(());
            return true;
        }
        if let Some(plan) = view.saved_merge_review.take() {
            view.review = Some(WritePlan::Merge(plan));
            view.error = None;
            return true;
        }
        if let Some(plan) = view.saved_mainline_review.take() {
            view.first = view
                .merge_review()
                .and_then(MergePlan::mainline)
                .map_or(0, |(number, _)| number - 1);
            view.review = Some(WritePlan::Merge(plan));
            view.merge_draft = MergeDraft::default();
            view.error = None;
            return true;
        }
        if matches!(view.draft, WriteDraft::Reset(..)) && view.reviewing() {
            view.review = None;
            view.first = 0;
            view.error = None;
            return true;
        }
        false
    }

    pub fn scroll(&mut self, delta: isize) {
        if self.busy() {
            return;
        }
        let Some(view) = &mut self.view else {
            return;
        };
        if let (Some(plan), Some(file)) = (
            view.review.as_ref().and_then(WritePlan::merge),
            &mut view.merge_draft.file,
        ) {
            view.error = file.scroll(plan, delta).err().map(str::to_owned);
            view.input_token = Arc::new(());
            return;
        }
        let count = view
            .repository_menu()
            .map(Iterator::count)
            .or_else(|| {
                view.repository_review()
                    .filter(|_| view.merge_conflicts().is_none())
                    .map(|plan| plan.rows.len())
            })
            .or_else(|| view.review().map(|plan| plan.files().len()))
            .or_else(|| view.branch_switch_review().map(|plan| plan.files().len()))
            .or_else(|| {
                view.merge_review().map(|plan| {
                    let steps = if plan.kind == MergeKind::Conflicts {
                        0
                    } else {
                        plan.rebase_steps().len()
                    };
                    (plan.rows.len() + steps)
                        .max(plan.mainline_choices().len())
                        .max(plan.rebase_todo().map_or(0, <[_]>::len))
                })
            })
            .or_else(|| view.merge_abort_review().map(|plan| plan.rows.len()))
            .unwrap_or(0);
        view.first = view
            .first
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }

    pub fn select_mainline(&mut self, index: usize) {
        if self.busy() {
            return;
        }
        if let Some(view) = self.view.as_mut().filter(|view| {
            view.choosing_mainline()
                .is_some_and(|plan| index < plan.mainline_choices().len())
        }) {
            view.first = index;
        }
    }

    pub fn edit_rebase_todo(&mut self, edit: RebaseTodoEdit) {
        if self.busy() {
            return;
        }
        let Some(view) = &mut self.view else {
            return;
        };
        let Some(WritePlan::Merge(plan)) = &mut view.review else {
            return;
        };
        let Some(todo) = plan.rebase_todo_mut() else {
            return;
        };
        match edit {
            RebaseTodoEdit::Select(index) if index < todo.len() => view.first = index,
            RebaseTodoEdit::Select(_) => return,
            RebaseTodoEdit::Move(delta) => {
                let next = view.first.saturating_add_signed(delta).min(todo.len() - 1);
                todo.swap(view.first, next);
                view.first = next;
            }
            RebaseTodoEdit::Action(action) => todo[view.first].action = action,
        }
        view.error = None;
    }

    pub fn submit(&mut self, bench: &Workbench, root: Option<&Path>) {
        if self.busy() {
            return;
        }
        let Some(view) = &mut self.view else {
            return;
        };
        if Some(view.root.as_path()) != root {
            self.cancel();
            return;
        }
        if let WriteDraft::RepositoryMenu(stash) = &view.draft {
            if let Some(action) = repository_actions(stash.is_some()).nth(view.first) {
                let stash = stash.clone().filter(|_| action.uses_stash());
                view.draft = WriteDraft::Repository(RepositoryDraft { action, stash });
                view.first = 0;
                view.input_token = Arc::new(());
            }
            return;
        }
        let identity_fields = match &view.draft {
            WriteDraft::Repository(draft)
                if view.review.is_none()
                    && view.secret.is_none()
                    && draft.action.accepts_network_credentials() =>
            {
                split_network_identity(&view.message, draft.action)
            }
            _ => None,
        };
        if let Some((target, identity)) = identity_fields {
            let mut fields = identity.split('|').map(str::trim);
            let username = fields.next().unwrap_or_default();
            let pin = fields.next();
            let private_key = fields.next().map(PathBuf::from);
            if username.is_empty()
                || username.len() > 256
                || username.contains(':')
                || pin.is_some_and(|pin| pin.is_empty() || pin.len() > 128)
                || private_key
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute() || path.as_os_str().len() > 4096)
                || fields.next().is_some()
            {
                view.error = Some("Especifica usuario, huella SHA256 para SSH y ruta absoluta de la clave si se usa".into());
                return;
            }
            let target = target.trim().to_owned();
            view.secret = Some(SecretEntry::new(
                username.to_owned(),
                pin.map(str::to_owned),
                private_key,
            ));
            view.message = target;
            view.cursor = view.message.len();
            view.input_token = Arc::new(());
            view.error = None;
            return;
        }
        if view
            .repository_review()
            .is_some_and(|review| !review.can_apply())
            && view.merge_conflicts().is_none()
        {
            return;
        }
        let mainline = if let Some(plan) = view.choosing_mainline() {
            let Some(review) = plan.mainline_review(view.first) else {
                return;
            };
            Some(review)
        } else {
            None
        };
        let todo = view.merge_review().and_then(MergePlan::rebase_todo_review);
        let resolution = if let Some(plan) = view.merge_conflicts() {
            if view.merge_file().is_some() || !view.all_merge_choices_saved() {
                return;
            }
            let review = if let Some(repository) = view.repository_review() {
                repository
                    .resolution_token()
                    .map(Box::new)
                    .map(ResolutionReview::Repository)
            } else {
                plan.resolution_review()
                    .map(Box::new)
                    .map(ResolutionReview::Merge)
            };
            let Some(review) = review else {
                return;
            };
            Some((review, view.merge_draft.choices.clone()))
        } else if mainline.is_none()
            && todo.is_none()
            && view.merge_review().is_some_and(|plan| !plan.can_apply())
        {
            return;
        } else {
            None
        };
        let preserves_worktree = matches!(
            view.draft,
            WriteDraft::Reset(ResetMode::Soft | ResetMode::Mixed, _)
        ) || (matches!(view.draft, WriteDraft::AbortMerge)
            && view.merge_abort_review().is_none_or(|plan| {
                matches!(plan.reset_mode(), Some(ResetMode::Soft | ResetMode::Mixed))
            }))
            || matches!(&view.draft, WriteDraft::Repository(draft) if !draft.action.changes_files());
        if !view.draft.is_reference()
            && !preserves_worktree
            && crate::git_index::require_clean(bench).is_err()
        {
            view.error = Some("Guarda los documentos modificados antes de continuar".into());
            return;
        }
        view.error = None;
        view.input_token = Arc::new(());
        if let Some(secret) = view.secret.take() {
            view.credentials = Some(Arc::new(secret.into_credentials()));
        }
        let plan = if resolution.is_some() || mainline.is_some() || todo.is_some() {
            None
        } else {
            view.saved_mainline_review = None;
            view.review.take()
        };
        let applying = plan.is_some();
        let root = view.root.clone();
        let message = view.message.clone();
        let replaces = view.replaces.clone();
        let draft = view.draft.clone();
        let credentials = view.credentials.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let result = Arc::new(Mutex::new(None));
        let slot = result.clone();
        let wake = self.wake.clone();
        let thread = thread::Builder::new()
            .name("cide-git-commit".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let cancelled = || stop.load(Ordering::Acquire);
                    if let Some(review) = todo {
                        return Completion::Review(
                            prepare_rebase_todo(review, cancelled)
                                .map(|plan| WritePlan::Merge(Box::new(plan))),
                        );
                    }
                    if let Some(review) = mainline {
                        return Completion::Review(
                            prepare_commit_mainline(review, cancelled)
                                .map(|plan| WritePlan::Merge(Box::new(plan))),
                        );
                    }
                    if let Some((review, choices)) = resolution {
                        return Completion::Review(match review {
                            ResolutionReview::Merge(review) => prepare_resolved_merge(*review, choices, cancelled)
                                .map(|plan| WritePlan::Merge(Box::new(plan))),
                            ResolutionReview::Repository(review) => review.prepare(choices, cancelled)
                                .map(|plan| WritePlan::Repository(Box::new(plan))),
                        });
                    }
                    match plan {
                        Some(WritePlan::Repository(plan)) => match plan.apply(credentials.as_deref(), cancelled) {
                            Ok(RepositoryOutcome::Done(notice)) => Completion::Applied(Ok(notice)),
                            Ok(RepositoryOutcome::PullReview(plan)) => Completion::PullReview(plan),
                            Err(error) => Completion::Applied(Err(error)),
                        },
                        Some(WritePlan::Commit(plan)) => {
                            Completion::Applied(apply_commit(*plan, cancelled).map(|publication| {
                                publication.reflog_warning.unwrap_or_else(|| {
                                    let label = if replaces.is_some() {
                                        "Commit modificado"
                                    } else {
                                        "Commit creado"
                                    };
                                    format!("{label} · {}", publication.id)
                                })
                            }))
                        }
                        Some(WritePlan::Reference(plan)) => {
                            Completion::Applied(apply_ref(*plan, cancelled).map(|publication| {
                                let label = match publication.kind {
                                    RefKind::Branch => "Rama creada",
                                    RefKind::Tag => "Etiqueta creada",
                                };
                                format!("{label} · {} · {}", publication.name, publication.target)
                            }))
                        }
                        Some(WritePlan::ReferenceChange(plan)) => Completion::Applied(
                            apply_ref_change(*plan, cancelled).map(|publication| match publication
                                .destination
                            {
                                Some(name) => format!(
                                    "Referencia renombrada · {} → {name}",
                                    publication.source
                                ),
                                None => format!("Referencia eliminada · {}", publication.source),
                            }),
                        ),
                        Some(WritePlan::Restore(plan)) => Completion::Applied(
                            apply_restore(*plan, cancelled).map(|publication| {
                                if let Some(backup) = publication.backup {
                                    let name = backup.file_name().unwrap_or(backup.as_os_str());
                                    format!(
                                        "Respaldo en .git/{} · Archivo restaurado",
                                        name.to_string_lossy()
                                    )
                                } else {
                                    "Archivo recuperado desde el índice Git".into()
                                }
                            }),
                        ),
                        Some(WritePlan::BranchSwitch(plan)) => Completion::Applied(
                            apply_branch_switch(*plan, cancelled).map(|publication| {
                                publication.reflog_warning.unwrap_or_else(|| {
                                    format!("Rama activa · {}", publication.destination)
                                })
                            }),
                        ),
                        Some(WritePlan::Merge(plan)) => {
                            let action = match plan.operation() {
                                Some(CommitOperation::CherryPick) => "Cherry-pick publicado",
                                Some(CommitOperation::Revert) => "Reversión publicada",
                                None if plan.reset_mode().is_some() => "Reset aplicado",
                                None if plan.is_rebase() => "Rebase aplicado",
                                None if matches!(&draft, WriteDraft::Repository(draft) if draft.action == RepositoryAction::Pull) => "Avance directo aplicado",
                                None => "Fusión local publicada",
                            };
                            Completion::Applied(apply_merge(*plan, cancelled).map(|publication| {
                                if let Some(name) = publication
                                    .hard_backup
                                    .as_ref()
                                    .and_then(|path| path.file_name())
                                {
                                    let notice = format!(
                                        "Reset hard aplicado · Respaldo en .git/{}",
                                        name.to_string_lossy()
                                    );
                                    return publication.reflog_warning.map_or_else(
                                        || notice.clone(),
                                        |warning| format!("{notice} · {warning}"),
                                    );
                                }
                                let mut message = publication
                                    .reflog_warning
                                    .unwrap_or_else(|| format!("{action} · {}", publication.id));
                                if let Some(name) = publication
                                    .index_backup
                                    .as_ref()
                                    .and_then(|path| path.file_name())
                                {
                                    message.push_str(&format!(
                                        " · Índice anterior en .git/{}",
                                        name.to_string_lossy()
                                    ));
                                }
                                message
                            }))
                        }
                        Some(WritePlan::MergeAbort(plan)) => {
                            let action = match plan.operation() {
                                Some(CommitOperation::CherryPick) => "Cherry-pick abortado",
                                Some(CommitOperation::Revert) => "Reversión abortada",
                                None if plan.reset_mode().is_some() => "Reset abortado",
                                None if plan.is_rebase() => "Rebase abortado",
                                None => "Fusión abortada",
                            };
                            Completion::Applied(apply_merge_abort(*plan, cancelled).map(|result| {
                                let mut notice =
                                    result.reflog_warning.unwrap_or_else(|| action.into());
                                if let Some(name) = result
                                    .hard_backup
                                    .as_ref()
                                    .and_then(|path| path.file_name())
                                {
                                    notice.push_str(&format!(
                                        " · Respaldo en .git/{}",
                                        name.to_string_lossy()
                                    ));
                                }
                                notice
                            }))
                        }
                        None => Completion::Review(match (draft, replaces) {
                            (WriteDraft::Repository(draft), _) => crate::git_repository::prepare(&root, draft, &message, credentials.as_deref(), LIMITS, cancelled)
                                .map(|plan| WritePlan::Repository(Box::new(plan))),
                            (WriteDraft::RepositoryMenu(_), _) => Err(jc_git::Error::Unsupported),
                            (WriteDraft::CreateReference(kind, target), _) => {
                                prepare_ref(&root, &target, kind, &message, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Reference(Box::new(plan)))
                            }
                            (WriteDraft::ChangeReference(target, rename), _) => prepare_ref_change(
                                &root,
                                target.kind,
                                &target.name,
                                &target.oid,
                                rename.then_some(message.as_str()),
                                LIMITS,
                                cancelled,
                            )
                            .map(|plan| WritePlan::ReferenceChange(Box::new(plan))),
                            (WriteDraft::SwitchBranch(target), _) => prepare_branch_switch(
                                &root,
                                &target.name,
                                &target.oid,
                                LIMITS,
                                cancelled,
                            )
                            .map(|plan| WritePlan::BranchSwitch(Box::new(plan))),
                            (WriteDraft::MergeBranch(target), _) => {
                                prepare_merge(&root, &target.name, &target.oid, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Merge(Box::new(plan)))
                            }
                            (WriteDraft::RebaseBranch(target, interactive), _) => {
                                let prepare = if interactive {
                                    prepare_interactive_rebase
                                } else {
                                    prepare_rebase
                                };
                                prepare(&root, &target.name, &target.oid, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Merge(Box::new(plan)))
                            }
                            (WriteDraft::CommitOperation(operation, commit), _) => {
                                prepare_commit_operation(
                                    &root, &commit, operation, LIMITS, cancelled,
                                )
                                .map(|plan| WritePlan::Merge(Box::new(plan)))
                            }
                            (WriteDraft::Reset(mode, commit), _) => {
                                prepare_reset(&root, &commit, mode, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Merge(Box::new(plan)))
                            }
                            (WriteDraft::AbortMerge, _) => {
                                prepare_merge_abort(&root, LIMITS, cancelled)
                                    .map(|plan| WritePlan::MergeAbort(Box::new(plan)))
                            }
                            (WriteDraft::Restore(comparison), _) => {
                                prepare_restore(&root, &comparison.path, LIMITS, cancelled)
                                    .and_then(|plan| {
                                        let pair = plan.pair();
                                        let ComparisonOrigin::Local(local) = &comparison.origin
                                        else {
                                            return Err(jc_git::Error::Stale);
                                        };
                                        if pair.repository.root != comparison.repository_root
                                            || pair.path != comparison.path
                                            || pair.index != local.index
                                            || pair.before.identity() != &local.before
                                            || pair.after.identity() != &local.after
                                        {
                                            return Err(jc_git::Error::Stale);
                                        }
                                        Ok(WritePlan::Restore(Box::new(plan)))
                                    })
                            }
                            (WriteDraft::Commit, Some(head)) => {
                                prepare_amend(&root, &message, &head, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Commit(Box::new(plan)))
                            }
                            (WriteDraft::Commit, None) => {
                                prepare_commit(&root, &message, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Commit(Box::new(plan)))
                            }
                        }),
                    }
                }))
                .unwrap_or(Completion::Panic);
                *slot.lock().unwrap() = Some(result);
                wake.signal();
            });
        match thread {
            Ok(thread) => {
                self.job = Some(Job {
                    result,
                    thread,
                    cancelled,
                    applying,
                })
            }
            Err(e) => view.error = Some(format!("No se pudo iniciar la tarea de Git ({e})")),
        }
    }

    /// Returns the result of a confirmed write independently of the current view.
    pub fn poll(&mut self) -> (bool, Option<CommitOutcome>) {
        let Some(job) = &self.job else {
            return (false, None);
        };
        let result = job.result.lock().unwrap().take();
        let Some(result) = result else {
            return (false, None);
        };
        let job = self.job.take().unwrap();
        let _ = job.thread.join();
        let Some(view) = &mut self.view else {
            return (true, None);
        };
        match result {
            Completion::PullReview(plan) => {
                view.credentials = None;
                view.review = Some(WritePlan::Merge(plan));
                view.first = 0;
                let root = view.root.clone();
                let notice = "Obtención completada · revisa el avance directo".to_owned();
                self.notice = Some((root.clone(), notice.clone()));
                return (
                    true,
                    Some(CommitOutcome {
                        root,
                        result: Ok(notice),
                        reference: true,
                        refresh_files: false,
                    }),
                );
            }
            Completion::Review(Ok(plan)) => {
                if matches!(&view.draft, WriteDraft::Repository(draft)
                    if matches!(draft.action, RepositoryAction::InitSubmodule | RepositoryAction::UpdateSubmodule))
                {
                    view.credentials = None;
                }
                if let Some(WritePlan::Merge(previous)) = view.review.replace(plan) {
                    if previous.is_rebase() {
                        // A new paused commit starts with its own path choices.
                        view.merge_draft = MergeDraft::default();
                    } else if previous.kind == MergeKind::ChooseMainline {
                        view.saved_mainline_review = Some(previous);
                    } else {
                        view.saved_merge_review = Some(previous);
                    }
                }
                view.first = 0;
            }
            Completion::Review(Err(e)) => {
                view.credentials = None;
                view.error = Some(e.to_string());
            }
            Completion::Applied(result) => {
                view.credentials = None;
                let root = view.root.clone();
                let reference = view.draft.refreshes_graph();
                let refresh_files = view.draft.refreshes_files();
                let result = result.map_err(|e| e.to_string());
                if let Ok(message) = &result {
                    self.notice = Some((root.clone(), message.clone()));
                    self.view = None;
                } else {
                    if view.draft.can_stop_publication() && job.cancelled.load(Ordering::Acquire) {
                        view.draft = WriteDraft::AbortMerge;
                        view.saved_merge_review = None;
                        view.merge_draft = MergeDraft::default();
                    } else if let Some(plan) = view.saved_merge_review.take() {
                        view.review = Some(WritePlan::Merge(plan));
                    }
                    view.error = result.as_ref().err().cloned();
                }
                return (
                    true,
                    Some(CommitOutcome {
                        root,
                        result,
                        reference,
                        refresh_files,
                    }),
                );
            }
            Completion::Panic => {
                view.credentials = None;
                view.error = Some(
                    if job.applying {
                        "Publicación incierta. Actualiza el repositorio antes de reintentar"
                    } else {
                        "Falló la preparación de la operación Git"
                    }
                    .into(),
                );
                if job.applying {
                    return (
                        true,
                        Some(CommitOutcome {
                            root: view.root.clone(),
                            result: Err(view.error.clone().unwrap()),
                            reference: view.draft.refreshes_graph(),
                            refresh_files: view.draft.refreshes_files(),
                        }),
                    );
                }
            }
        }
        (true, None)
    }
}

pub struct CommitOutcome {
    pub root: PathBuf,
    pub result: Result<String, String>,
    pub reference: bool,
    pub refresh_files: bool,
}

#[derive(Clone, Copy)]
pub enum RebaseTodoEdit {
    Select(usize),
    Move(isize),
    Action(RebaseAction),
}

pub fn rebase_todo_label(item: &jc_git::local_merge::RebaseTodo) -> String {
    let action = match item.action {
        RebaseAction::Pick => "Reaplicar",
        RebaseAction::Squash => "Combinar",
        RebaseAction::Drop => "Omitir",
    };
    format!(
        "{action} · {} · {}",
        item.original.to_hex_with_len(8),
        item.subject
    )
}

pub fn rebase_step_label(step: &RebaseStep) -> String {
    let original = step.original.to_hex_with_len(8);
    match step.state {
        RebaseStepState::Pending => format!("Pendiente · {original}"),
        RebaseStepState::Replayed(id) => {
            format!("Reaplicar · {original} → {}", id.to_hex_with_len(8))
        }
        RebaseStepState::AlreadyApplied => format!("Ya aplicado · {original}"),
        RebaseStepState::Omitted => format!("Omitido · {original}"),
        RebaseStepState::Combined(id) => {
            format!("Combinado · {original} → {}", id.to_hex_with_len(8))
        }
        RebaseStepState::Conflict => format!("Conflicto · {original}"),
    }
}

pub fn file_flag(change: jc_git::Change) -> &'static str {
    match change {
        jc_git::Change::Added => "+",
        jc_git::Change::Deleted => "−",
        _ => "∼",
    }
}

impl Drop for CommitController {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            if !job.applying {
                job.cancelled.store(true, Ordering::Release);
            }
            let _ = job.thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_credential_is_separate_from_metadata_and_masked_until_cancelled() {
        let root = PathBuf::from("repo-fixture");
        let mut controller = CommitController::new(Wakeup::default());
        controller.open_worktree_clone(root.clone());
        controller.edit(FieldEdit::Insert(
            "https://example.invalid/repo | C:/destino | usuario",
        ));
        controller.submit(&Workbench::default(), Some(&root));
        assert!(controller.view().unwrap().presenting_secret());
        assert!(!controller.busy());
        controller.edit(FieldEdit::Insert("secreto-sintético"));
        let view = controller.view().unwrap();
        assert_eq!(view.message, "https://example.invalid/repo | C:/destino");
        let shown = view.present_input(80).rows.join("");
        assert!(!shown.contains("secreto"));
        assert!(shown.contains('*'));
        controller.edit(FieldEdit::Backspace);
        assert_eq!(
            controller.view().unwrap().secret.as_ref().unwrap().text(),
            "secreto-sintétic"
        );
        controller.cancel();
        assert!(controller.view().is_none());
    }
}
