//! Repository actions shared by the graphical and terminal hosts.
use jc_git::{
    local_merge::{prepare_tracking_fast_forward, MergePlan, MergeResolution},
    remote, remote_config_write as config, stash, submodules, worktrees, Error, Limits,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryAction {
    StashSave,
    StashApply,
    StashPop,
    StashDrop,
    Remotes,
    AddRemote,
    RemoveRemote,
    Fetch,
    Pull,
    Push,
    Clone,
    Upstream,
    Worktrees,
    LockWorktree,
    UnlockWorktree,
    Submodules,
}

impl RepositoryAction {
    pub fn title(self) -> &'static str {
        match self {
            Self::StashSave => "Guardar cambios en una reserva",
            Self::StashApply => "Aplicar reserva",
            Self::StashPop => "Aplicar y retirar reserva",
            Self::StashDrop => "Eliminar reserva",
            Self::Remotes => "Ver remotos y seguimiento",
            Self::AddRemote => "Agregar remoto",
            Self::RemoveRemote => "Quitar configuración remota",
            Self::Fetch => "Obtener cambios (fetch)",
            Self::Pull => "Obtener e integrar (pull)",
            Self::Push => "Enviar cambios (push)",
            Self::Clone => "Clonar repositorio bare local",
            Self::Upstream => "Configurar seguimiento",
            Self::Worktrees => "Ver árboles de trabajo",
            Self::LockWorktree => "Bloquear árbol de trabajo",
            Self::UnlockWorktree => "Desbloquear árbol de trabajo",
            Self::Submodules => "Ver submódulos",
        }
    }

    pub fn prompt(self) -> Option<&'static str> {
        match self {
            Self::StashSave => Some("Mensaje opcional para identificar la reserva"),
            Self::Fetch | Self::Pull | Self::Push => {
                Some("Nombre del remoto · vacío usa el configurado")
            }
            Self::Clone => Some("Ruta del repositorio bare | carpeta nueva de destino"),
            Self::Upstream => Some("Remoto | rama · vacío elimina el seguimiento"),
            Self::AddRemote => Some("Nombre | ruta o URL sin credenciales"),
            Self::RemoveRemote => Some("Nombre del remoto"),
            Self::LockWorktree => Some("Identificador del árbol | motivo opcional"),
            Self::UnlockWorktree => Some("Identificador del árbol"),
            _ => None,
        }
    }

    pub fn changes_files(self) -> bool {
        matches!(
            self,
            Self::StashSave | Self::StashApply | Self::StashPop | Self::Pull
        )
    }

    pub fn uses_stash(self) -> bool {
        matches!(self, Self::StashApply | Self::StashPop | Self::StashDrop)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashTarget {
    pub index: usize,
    pub oid: String,
}

#[derive(Clone)]
pub struct RepositoryDraft {
    pub action: RepositoryAction,
    pub stash: Option<StashTarget>,
}

pub struct RepositoryReview {
    pub rows: Vec<String>,
    publication: Option<Publication>,
}

enum Publication {
    Config(config::Review),
    Lock(worktrees::LockReview),
    Unlock(worktrees::UnlockReview),
    Fetch(remote::FetchPlan),
    Pull {
        fetch: remote::FetchPlan,
        root: PathBuf,
        tracking: String,
        tip: String,
        limits: Limits,
    },
    Push(remote::PushPlan),
    Clone(remote::LocalClonePlan),
    SaveStash(Box<stash::StashPushPlan>),
    ApplyStash(Box<stash::StashApplyPlan>),
    PopStash(Box<stash::StashPopPlan>),
    DropStash(Box<stash::StashDropPlan>),
}

pub(crate) enum RepositoryOutcome {
    Done(String),
    PullReview(Box<MergePlan>),
}

impl RepositoryReview {
    fn inspection(rows: Vec<String>) -> Self {
        Self {
            rows,
            publication: None,
        }
    }

    fn mutation(rows: Vec<String>, publication: Publication) -> Self {
        Self {
            rows,
            publication: Some(publication),
        }
    }

    pub fn can_apply(&self) -> bool {
        match &self.publication {
            Some(Publication::ApplyStash(plan)) => plan.can_apply(),
            Some(Publication::PopStash(plan)) => plan.can_apply(),
            other => other.is_some(),
        }
    }

    pub(crate) fn merge_plan(&self) -> Option<&MergePlan> {
        match &self.publication {
            Some(Publication::ApplyStash(plan)) => Some(plan.merge_plan()),
            Some(Publication::PopStash(plan)) => Some(plan.merge_plan()),
            _ => None,
        }
    }

    pub(crate) fn resolution_token(&self) -> Option<RepositoryResolution> {
        let (apply, pop) = match &self.publication {
            Some(Publication::ApplyStash(plan)) => (plan.resolution_token()?, false),
            Some(Publication::PopStash(plan)) => (plan.resolution_token()?, true),
            _ => return None,
        };
        Some(RepositoryResolution { apply, pop })
    }

    pub(crate) fn apply(
        self,
        mut cancelled: impl FnMut() -> bool + Send,
    ) -> Result<RepositoryOutcome, Error> {
        match self.publication.ok_or(Error::Unsupported)? {
            Publication::Config(review) => {
                config::apply(review, cancelled)?;
                Ok("Configuración de Git actualizada".into())
            }
            Publication::Lock(review) => {
                worktrees::apply_lock(review, cancelled)?;
                Ok("Árbol de trabajo bloqueado".into())
            }
            Publication::Unlock(review) => {
                worktrees::apply_unlock(review, cancelled)?;
                Ok("Árbol de trabajo desbloqueado".into())
            }
            Publication::Fetch(plan) => remote::apply_local_fetch(plan, cancelled)
                .map(|result| transfer_result("Obtención completada", result)),
            Publication::Pull {
                fetch,
                root,
                tracking,
                tip,
                limits,
            } => {
                let fetched = remote::apply_local_fetch(fetch, &mut cancelled)?;
                return Ok(
                    match prepare_tracking_fast_forward(&root, &tracking, &tip, limits, cancelled) {
                        Ok(plan) if plan.can_apply() => {
                            RepositoryOutcome::PullReview(Box::new(plan))
                        }
                        Ok(_) => RepositoryOutcome::Done(transfer_result(
                            "Obtención completada · rama al día",
                            fetched,
                        )),
                        Err(error) => RepositoryOutcome::Done(format!(
                            "{} · integración pendiente: {error}",
                            transfer_result("Obtención completada", fetched),
                        )),
                    },
                );
            }
            Publication::Push(plan) => remote::apply_local_push(plan, cancelled)
                .map(|result| transfer_result("Envío completado", result)),
            Publication::Clone(plan) => remote::apply_local_bare_clone(plan, cancelled)
                .map(|result| transfer_result("Clon bare creado", result)),
            Publication::SaveStash(plan) => stash::apply_push(*plan, cancelled).map(|result| {
                let mut message = if result.workspace_clean {
                    format!("Reserva guardada · {}", result.oid)
                } else {
                    format!("Reserva guardada · {} · limpieza pendiente", result.oid)
                };
                append_warning(&mut message, result.warning);
                message
            }),
            Publication::ApplyStash(plan) => stash::apply_stash(*plan, cancelled).map(|result| {
                let mut message = format!("Reserva aplicada · {}", result.oid);
                append_warning(&mut message, result.warning);
                message
            }),
            Publication::PopStash(plan) => stash::apply_pop(*plan, cancelled).map(|result| {
                let label = if result.dropped {
                    "Reserva aplicada y retirada"
                } else {
                    "Reserva aplicada · retiro sin confirmar"
                };
                let mut message = format!("{label} · {}", result.applied.oid);
                append_warning(&mut message, result.applied.warning);
                append_warning(&mut message, result.warning);
                message
            }),
            Publication::DropStash(plan) => stash::apply_drop(*plan, cancelled).map(|result| {
                let label = if result.warning.is_none() {
                    "Reserva eliminada"
                } else {
                    "Retiro de reserva sin confirmar"
                };
                let mut message = format!("{label} · {}", result.oid);
                append_warning(&mut message, result.warning);
                message
            }),
        }
        .map(RepositoryOutcome::Done)
    }
}

pub(crate) fn prepare(
    root: &Path,
    draft: RepositoryDraft,
    input: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RepositoryReview, Error> {
    use RepositoryAction::*;
    match draft.action {
        StashSave => {
            let plan = stash::prepare_push(root, input.trim(), limits, cancelled)?;
            let rows = stash_rows(plan.stash_oid().to_string(), plan.paths());
            Ok(RepositoryReview::mutation(
                rows,
                Publication::SaveStash(Box::new(plan)),
            ))
        }
        StashApply | StashPop | StashDrop => {
            let target = draft.stash.ok_or(Error::Stale)?;
            let selector = stash::StashSelector {
                index: target.index,
                oid: target.oid.parse().map_err(|_| Error::Stale)?,
            };
            match draft.action {
                StashApply => {
                    stash::prepare_apply(root, selector, limits, cancelled).map(review_stash_apply)
                }
                StashPop => {
                    stash::prepare_pop(root, selector, limits, cancelled).map(review_stash_pop)
                }
                StashDrop => {
                    let plan = stash::prepare_drop(root, selector, limits, cancelled)?;
                    let rows = vec![format!(
                        "Retirar stash@{{{}}} · {}",
                        selector.index,
                        plan.stash_oid()
                    )];
                    Ok(RepositoryReview::mutation(
                        rows,
                        Publication::DropStash(Box::new(plan)),
                    ))
                }
                _ => unreachable!(),
            }
        }
        Remotes => {
            let snapshot = remote::read_remotes(root, limits, cancelled)?;
            let mut rows = Vec::with_capacity(snapshot.remotes.len() + 1);
            if let Some(branch) = snapshot.active_branch {
                rows.push(format!("Rama activa · {}", safe_label(&branch)));
            }
            if snapshot.remotes.is_empty() {
                rows.push("El repositorio no tiene remotos configurados".into());
            }
            rows.extend(snapshot.remotes.into_iter().map(|remote| {
                let endpoint = remote.host.as_deref().map_or_else(
                    || safe_label(&remote.transport),
                    |host| format!("{} · {}", safe_label(&remote.transport), safe_label(host)),
                );
                let tracking = remote
                    .active_upstream
                    .as_deref()
                    .map_or(String::new(), |name| {
                        format!(" · sigue {}", safe_label(name))
                    });
                format!("{} · {endpoint}{tracking}", safe_label(&remote.name))
            }));
            Ok(RepositoryReview::inspection(rows))
        }
        Fetch => {
            let plan = remote::prepare_configured_fetch(root, input.trim(), limits, cancelled)?;
            let rows = transfer_rows(remote::fetch_preview(&plan));
            Ok(RepositoryReview::mutation(rows, Publication::Fetch(plan)))
        }
        Pull => prepare_pull(root, input.trim(), limits, cancelled),
        Push => {
            let plan = remote::prepare_configured_push(root, input.trim(), limits, cancelled)?;
            let rows = transfer_rows(remote::push_preview(&plan));
            Ok(RepositoryReview::mutation(rows, Publication::Push(plan)))
        }
        Clone => {
            let (source, destination) = fields(input)?;
            let plan = remote::prepare_local_bare_clone(
                Path::new(source),
                Path::new(destination),
                "origin",
                limits,
                cancelled,
            )?;
            let mut rows = vec![format!(
                "{} → {}",
                safe_label(source),
                safe_label(destination)
            )];
            rows.extend(transfer_rows(remote::clone_preview(&plan)));
            Ok(RepositoryReview::mutation(rows, Publication::Clone(plan)))
        }
        AddRemote | RemoveRemote | Upstream => {
            let change = match draft.action {
                AddRemote => {
                    let (name, location) = fields(input)?;
                    config::Change::AddRemote { name, location }
                }
                RemoveRemote => config::Change::RemoveRemote { name: input.trim() },
                Upstream if input.trim().is_empty() => config::Change::ClearUpstream,
                Upstream => {
                    let (remote, branch) = fields(input)?;
                    config::Change::SetUpstream { remote, branch }
                }
                _ => unreachable!(),
            };
            let review = config::prepare(root, change, limits, cancelled)?;
            Ok(RepositoryReview::mutation(
                review.rows().to_vec(),
                Publication::Config(review),
            ))
        }
        Worktrees => {
            let inventory = worktrees::list(root, limits, cancelled)?;
            let rows = inventory
                .worktrees
                .into_iter()
                .map(|tree| {
                    let name = tree.id.as_deref().unwrap_or("Principal");
                    let location = safe_label(&tree.path.to_string_lossy());
                    let branch = tree
                        .branch
                        .as_deref()
                        .or(tree.head.as_deref())
                        .unwrap_or("sin commits");
                    let lock = if tree.locked { " · bloqueado" } else { "" };
                    let unavailable = if tree.prunable {
                        " · registro sin carpeta"
                    } else {
                        ""
                    };
                    let reason = tree
                        .lock_reason
                        .as_deref()
                        .filter(|reason| !reason.is_empty())
                        .map_or(String::new(), |reason| format!(" · {}", safe_label(reason)));
                    format!(
                        "{name} · {location} · {}{lock}{unavailable}{reason}",
                        safe_label(branch)
                    )
                })
                .collect();
            Ok(RepositoryReview::inspection(rows))
        }
        LockWorktree => {
            let (id, reason) = input.split_once('|').unwrap_or((input, ""));
            let review =
                worktrees::prepare_lock(root, id.trim(), reason.trim(), limits, cancelled)?;
            let rows = vec![
                format!("Bloquear {}", safe_label(review.id())),
                safe_label(review.reason()),
            ];
            Ok(RepositoryReview::mutation(rows, Publication::Lock(review)))
        }
        UnlockWorktree => {
            let review = worktrees::prepare_unlock(root, input.trim(), limits, cancelled)?;
            let rows = vec![
                format!("Desbloquear {}", safe_label(review.id())),
                safe_label(review.reason()),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::Unlock(review),
            ))
        }
        Submodules => {
            let inventory = submodules::list(root, limits, cancelled)?;
            let rows = if inventory.submodules.is_empty() {
                vec!["El commit actual no contiene submódulos".into()]
            } else {
                inventory
                    .submodules
                    .into_iter()
                    .map(|module| {
                        let path = safe_label(&String::from_utf8_lossy(&module.path));
                        let registration = if module.registered {
                            ""
                        } else {
                            " · sin registro en .gitmodules"
                        };
                        format!("{path} · {}{registration}", module.object_id)
                    })
                    .collect()
            };
            Ok(RepositoryReview::inspection(rows))
        }
    }
}

fn prepare_pull(
    root: &Path,
    requested: &str,
    limits: Limits,
    mut cancelled: impl FnMut() -> bool + Send,
) -> Result<RepositoryReview, Error> {
    let snapshot = remote::read_remotes(root, limits, &mut cancelled)?;
    let remote = snapshot
        .remotes
        .into_iter()
        .find(|item| {
            (requested.is_empty() || requested == item.name) && item.active_upstream.is_some()
        })
        .ok_or_else(|| {
            Error::Engine("Configura el seguimiento de la rama activa para usar pull".into())
        })?;
    let tracking = remote.active_upstream.ok_or(Error::Stale)?;
    let fetch = remote::prepare_configured_fetch(root, &remote.name, limits, cancelled)?;
    let preview = remote::fetch_preview(&fetch);
    let tip = preview
        .updates
        .iter()
        .find(|update| update.destination == tracking)
        .ok_or_else(|| Error::Engine("El remoto no contiene la rama de seguimiento".into()))?
        .new
        .clone();
    let mut rows = vec![format!(
        "Tras obtener, revisa el avance directo desde {}",
        safe_label(&tracking)
    )];
    rows.extend(transfer_rows(preview));
    Ok(RepositoryReview::mutation(
        rows,
        Publication::Pull {
            fetch,
            root: root.to_owned(),
            tracking,
            tip,
            limits,
        },
    ))
}

pub(crate) struct RepositoryResolution {
    apply: stash::StashApplyResolution,
    pop: bool,
}

impl RepositoryResolution {
    pub(crate) fn prepare(
        self,
        choices: BTreeMap<Vec<u8>, MergeResolution>,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<RepositoryReview, Error> {
        let apply = self.apply.prepare(choices, cancelled)?;
        if self.pop {
            Ok(review_stash_pop(stash::StashPopPlan::from_resolved(apply)))
        } else {
            Ok(review_stash_apply(apply))
        }
    }
}

fn review_stash_apply(plan: stash::StashApplyPlan) -> RepositoryReview {
    let rows = stash_rows(plan.stash_oid().to_string(), plan.paths());
    RepositoryReview::mutation(rows, Publication::ApplyStash(Box::new(plan)))
}

fn review_stash_pop(plan: stash::StashPopPlan) -> RepositoryReview {
    let rows = stash_rows(plan.stash_oid().to_string(), plan.paths());
    RepositoryReview::mutation(rows, Publication::PopStash(Box::new(plan)))
}

fn stash_rows<'a>(oid: String, paths: impl Iterator<Item = &'a [u8]>) -> Vec<String> {
    let mut rows = vec![format!("Reserva · {oid}")];
    rows.extend(paths.map(|path| safe_label(&String::from_utf8_lossy(path))));
    rows
}

fn append_warning(message: &mut String, warning: Option<String>) {
    if let Some(warning) = warning {
        message.push_str(" · ");
        message.push_str(&warning);
    }
}

fn transfer_rows(preview: &remote::TransferPreview) -> Vec<String> {
    let mut rows = Vec::with_capacity(preview.updates.len() + 2);
    rows.push(format!(
        "{} · {} objetos · {} bytes",
        safe_label(&preview.remote),
        preview.object_count,
        preview.object_bytes
    ));
    if let Some(path) = &preview.destination_path {
        rows.push(format!("Destino · {}", safe_label(path)));
    }
    rows.extend(preview.updates.iter().map(|update| {
        format!(
            "{} → {} · {} → {}",
            safe_label(&update.source),
            safe_label(&update.destination),
            update.old.as_deref().unwrap_or("ausente"),
            update.new,
        )
    }));
    rows
}

fn transfer_result(label: &str, result: remote::TransferResult) -> String {
    format!(
        "{label} · {} · {} referencias actualizadas",
        safe_label(&result.remote),
        result.updated.len()
    )
}

fn fields(input: &str) -> Result<(&str, &str), Error> {
    let (left, right) = input
        .split_once('|')
        .ok_or_else(|| Error::Engine("Separa los campos con |".into()))?;
    let (left, right) = (left.trim(), right.trim());
    if left.is_empty() || right.is_empty() {
        return Err(Error::Engine("Completa ambos campos".into()));
    }
    Ok((left, right))
}

fn safe_label(value: &str) -> String {
    crate::source_services::display_ui_label(value, 256)
}

pub fn repository_actions(stash: bool) -> impl Iterator<Item = RepositoryAction> {
    use RepositoryAction::*;
    [
        StashSave,
        StashApply,
        StashPop,
        StashDrop,
        Remotes,
        Fetch,
        Pull,
        Push,
        Clone,
        AddRemote,
        RemoveRemote,
        Upstream,
        Worktrees,
        LockWorktree,
        UnlockWorktree,
        Submodules,
    ]
    .into_iter()
    .filter(move |action| {
        (stash || !action.uses_stash())
            && (cfg!(windows) || matches!(action, Remotes | Fetch | Push | Worktrees | Submodules))
    })
}
