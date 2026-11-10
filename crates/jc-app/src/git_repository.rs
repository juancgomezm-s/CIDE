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
    CloneWorktree,
    Upstream,
    Worktrees,
    CreateWorktree,
    ResumeWorktree,
    MoveWorktree,
    ResumeMoveWorktree,
    RetireWorktree,
    RestoreRetiredWorktree,
    LockWorktree,
    UnlockWorktree,
    Submodules,
    InitSubmodule,
    UpdateSubmodule,
    RecoverSubmodule,
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
            Self::CloneWorktree => "Clonar con directorio de trabajo",
            Self::Upstream => "Configurar seguimiento",
            Self::Worktrees => "Ver árboles de trabajo",
            Self::CreateWorktree => "Crear árbol de trabajo",
            Self::ResumeWorktree => "Reanudar creación de árbol",
            Self::MoveWorktree => "Trasladar árbol de trabajo",
            Self::ResumeMoveWorktree => "Reanudar traslado de árbol",
            Self::RetireWorktree => "Retirar árbol de trabajo",
            Self::RestoreRetiredWorktree => "Restaurar árbol retirado",
            Self::LockWorktree => "Bloquear árbol de trabajo",
            Self::UnlockWorktree => "Desbloquear árbol de trabajo",
            Self::Submodules => "Ver submódulos",
            Self::InitSubmodule => "Inicializar submódulo",
            Self::UpdateSubmodule => "Actualizar submódulo",
            Self::RecoverSubmodule => "Reanudar operación de submódulo",
        }
    }

    pub fn prompt(self) -> Option<&'static str> {
        match self {
            Self::StashSave => Some("Mensaje opcional para identificar la reserva"),
            Self::Fetch | Self::Pull | Self::Push => {
                Some("Remoto opcional · credenciales: remoto | usuario | huella SSH | clave")
            }
            Self::Clone => Some("Ruta del repositorio bare | carpeta nueva de destino"),
            Self::CloneWorktree => Some(
                "Origen (ruta o URL) | carpeta nueva · opcional: | usuario | huella SSH | clave",
            ),
            Self::Upstream => Some("Remoto | rama · vacío elimina el seguimiento"),
            Self::AddRemote => Some("Nombre | ruta o URL sin credenciales"),
            Self::RemoveRemote => Some("Nombre del remoto"),
            Self::CreateWorktree => Some("Rama (refs/heads/…) o SHA completo | carpeta nueva"),
            Self::ResumeWorktree => Some("Identificador del árbol incompleto"),
            Self::MoveWorktree => Some("Identificador del árbol | carpeta nueva de destino"),
            Self::ResumeMoveWorktree => Some("Identificador del árbol en traslado"),
            Self::RetireWorktree | Self::RestoreRetiredWorktree => Some("Identificador del árbol"),
            Self::LockWorktree => Some("Identificador del árbol | motivo opcional"),
            Self::UnlockWorktree => Some("Identificador del árbol"),
            Self::InitSubmodule | Self::UpdateSubmodule => {
                Some("Ruta del submódulo · opcional: | SHA | usuario | huella SSH | clave")
            }
            _ => None,
        }
    }

    pub fn changes_files(self) -> bool {
        matches!(
            self,
            Self::StashSave
                | Self::StashApply
                | Self::StashPop
                | Self::Pull
                | Self::InitSubmodule
                | Self::MoveWorktree
                | Self::ResumeMoveWorktree
                | Self::RetireWorktree
                | Self::RestoreRetiredWorktree
                | Self::UpdateSubmodule
                | Self::RecoverSubmodule
        )
    }

    pub fn accepts_network_credentials(self) -> bool {
        matches!(
            self,
            Self::Fetch
                | Self::Pull
                | Self::Push
                | Self::CloneWorktree
                | Self::InitSubmodule
                | Self::UpdateSubmodule
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
    CreateWorktree(Box<worktrees::CreateReview>),
    ResumeWorktree(Box<worktrees::ResumeCreateReview>),
    MoveWorktree(Box<worktrees::MoveReview>),
    ResumeMoveWorktree(Box<worktrees::ResumeMoveReview>),
    RetireWorktree(Box<worktrees::RetireReview>),
    RestoreRetiredWorktree(Box<worktrees::RestoreRetiredReview>),
    Lock(worktrees::LockReview),
    Unlock(worktrees::UnlockReview),
    InitSubmodule(Box<submodules::InitPlan>),
    UpdateSubmodule(Box<submodules::UpdatePlan>),
    RecoverSubmodule(PathBuf, SubmoduleRecovery, Limits),
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
    NetworkClone(Box<remote::NetworkClonePlan>),
    SaveStash(Box<stash::StashPushPlan>),
    ApplyStash(Box<stash::StashApplyPlan>),
    PopStash(Box<stash::StashPopPlan>),
    DropStash(Box<stash::StashDropPlan>),
}

enum SubmoduleRecovery {
    Init(submodules::InitPreview),
    Update(submodules::UpdatePreview),
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
        credential: Option<&remote::OperationCredential>,
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
            Publication::InitSubmodule(plan) => submodules::apply_init(*plan, cancelled)
                .map(|preview| format!("Submódulo inicializado · {}", safe_label(&preview.path))),
            Publication::UpdateSubmodule(plan) => submodules::apply_update(*plan, cancelled)
                .map(|preview| format!("Submódulo actualizado · {}", safe_label(&preview.path))),
            Publication::RecoverSubmodule(root, recovery, limits) => {
                let resumed = match recovery {
                    SubmoduleRecovery::Init(preview) => {
                        submodules::recover_init_expected(&root, &preview, limits, cancelled)?
                    }
                    SubmoduleRecovery::Update(preview) => {
                        submodules::recover_update_expected(&root, &preview, limits, cancelled)?
                    }
                };
                if !resumed {
                    return Err(Error::Stale);
                }
                Ok("Operación de submódulo completada".into())
            }
            Publication::Fetch(plan) => apply_fetch(plan, credential, cancelled)
                .map(|result| transfer_result("Obtención completada", result)),
            Publication::Pull {
                fetch,
                root,
                tracking,
                tip,
                limits,
            } => {
                let fetched = apply_fetch(fetch, credential, &mut cancelled)?;
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
            Publication::CreateWorktree(review) => {
                worktrees::apply_create(*review, cancelled).map(|tree| {
                    format!(
                        "Árbol de trabajo creado · {}",
                        safe_label(&tree.path.to_string_lossy())
                    )
                })
            }
            Publication::ResumeWorktree(review) => {
                worktrees::apply_resume_create(*review, cancelled).map(|tree| {
                    format!(
                        "Árbol de trabajo completado · {}",
                        safe_label(&tree.path.to_string_lossy())
                    )
                })
            }
            Publication::MoveWorktree(review) => {
                worktrees::apply_move(*review, cancelled).map(|tree| worktree_moved(&tree.path))
            }
            Publication::ResumeMoveWorktree(review) => {
                worktrees::apply_resume_move(*review, cancelled)
                    .map(|tree| worktree_moved(&tree.path))
            }
            Publication::RetireWorktree(review) => {
                worktrees::apply_retire(*review, cancelled).map(|tree| {
                    format!(
                        "Árbol retirado · archivos conservados en {}",
                        safe_label(&tree.worktree.to_string_lossy())
                    )
                })
            }
            Publication::RestoreRetiredWorktree(review) => {
                worktrees::apply_restore_retired(*review, cancelled).map(|tree| {
                    format!(
                        "Árbol restaurado · {}",
                        safe_label(&tree.path.to_string_lossy())
                    )
                })
            }
            Publication::Push(plan) => {
                let result = if remote::push_preview(&plan).file_transport {
                    remote::apply_local_push(plan, cancelled)?
                } else {
                    remote::apply_network_push(plan, credential, cancelled)?
                };
                Ok(transfer_result("Envío completado", result))
            }
            Publication::Clone(plan) => {
                if remote::clone_checkout(&plan).is_some() {
                    remote::apply_local_worktree_clone(plan, cancelled)
                        .map(|result| transfer_result("Clon creado", result))
                } else {
                    remote::apply_local_bare_clone(plan, cancelled)
                        .map(|result| transfer_result("Clon bare creado", result))
                }
            }
            Publication::NetworkClone(plan) => {
                let clone = remote::apply_network_worktree_clone(*plan, credential, cancelled)?;
                Ok(format!(
                    "Clon creado · {} · {} archivos",
                    safe_label(&clone.destination.to_string_lossy()),
                    clone.files
                ))
            }
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
    credential: Option<&remote::OperationCredential>,
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
            let plan = prepare_fetch(root, input.trim(), credential, limits, cancelled)?;
            let rows = transfer_rows(remote::fetch_preview(&plan));
            Ok(RepositoryReview::mutation(rows, Publication::Fetch(plan)))
        }
        Pull => prepare_pull(root, input.trim(), credential, limits, cancelled),
        Push => {
            let mut cancelled = cancelled;
            let name = input.trim();
            let plan = if remote::configured_remote_is_network(
                root,
                name,
                true,
                limits,
                &mut cancelled,
            )? {
                remote::prepare_network_push(root, name, credential, limits, cancelled)?
            } else {
                remote::prepare_configured_push(root, name, limits, cancelled)?
            };
            let rows = transfer_rows(remote::push_preview(&plan));
            Ok(RepositoryReview::mutation(rows, Publication::Push(plan)))
        }
        Clone | CloneWorktree => {
            let (source, destination) = fields(input)?;
            if draft.action == CloneWorktree && !Path::new(source).is_absolute() {
                let plan = remote::prepare_network_worktree_clone(
                    source,
                    Path::new(destination),
                    limits,
                    cancelled,
                )?;
                let rows = vec![
                    format!("Origen · {}", safe_label(plan.source_label())),
                    format!(
                        "Destino nuevo · {}",
                        safe_label(&plan.destination().to_string_lossy())
                    ),
                    "La confirmación obtiene el repositorio y abre su rama predeterminada".into(),
                ];
                return Ok(RepositoryReview::mutation(
                    rows,
                    Publication::NetworkClone(Box::new(plan)),
                ));
            }
            let prepare = if draft.action == CloneWorktree {
                remote::prepare_local_worktree_clone
            } else {
                remote::prepare_local_bare_clone
            };
            let plan = prepare(
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
            if let Some((files, bytes)) = remote::clone_checkout(&plan) {
                rows.push(format!(
                    "Directorio de trabajo · {files} archivos · {bytes} bytes"
                ));
            }
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
        CreateWorktree => {
            let (target, destination) = fields(input)?;
            let review =
                worktrees::prepare_create(root, Path::new(destination), target, limits, cancelled)?;
            let rows = vec![
                format!(
                    "Destino · {}",
                    safe_label(&review.destination().to_string_lossy())
                ),
                format!("Commit · {}", safe_label(review.commit())),
                format!(
                    "Rama · {}",
                    safe_label(review.branch().unwrap_or("HEAD separado"))
                ),
                format!(
                    "{} archivos · {} bytes",
                    review.file_count(),
                    review.byte_count()
                ),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::CreateWorktree(Box::new(review)),
            ))
        }
        ResumeWorktree => {
            let review = worktrees::prepare_resume_create(root, input.trim(), limits, cancelled)?;
            let rows = vec![
                format!("Árbol · {}", safe_label(review.id())),
                format!("Commit · {}", safe_label(review.commit())),
                format!(
                    "{} archivos · {} bytes",
                    review.file_count(),
                    review.byte_count()
                ),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::ResumeWorktree(Box::new(review)),
            ))
        }
        MoveWorktree => {
            let (id, destination) = fields(input)?;
            let review =
                worktrees::prepare_move(root, id, Path::new(destination), limits, cancelled)?;
            let rows = worktree_move_rows(review.id(), review.source(), review.destination());
            Ok(RepositoryReview::mutation(
                rows,
                Publication::MoveWorktree(Box::new(review)),
            ))
        }
        ResumeMoveWorktree => {
            let review = worktrees::prepare_resume_move(root, input.trim(), limits, cancelled)?;
            let rows = worktree_move_rows(review.id(), review.source(), review.destination());
            Ok(RepositoryReview::mutation(
                rows,
                Publication::ResumeMoveWorktree(Box::new(review)),
            ))
        }
        RetireWorktree => {
            let review = worktrees::prepare_retire(root, input.trim(), limits, cancelled)?;
            let mut rows =
                worktree_move_rows(review.id(), review.worktree(), review.quarantine_worktree());
            rows.push("Se conservan los archivos y el índice para restaurarlos".into());
            Ok(RepositoryReview::mutation(
                rows,
                Publication::RetireWorktree(Box::new(review)),
            ))
        }
        RestoreRetiredWorktree => {
            let review = worktrees::prepare_restore_retired(root, input.trim(), limits, cancelled)?;
            let rows = vec![
                format!("Árbol · {}", safe_label(review.id())),
                format!(
                    "Ubicación original · {}",
                    safe_label(&review.original_worktree().to_string_lossy())
                ),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::RestoreRetiredWorktree(Box::new(review)),
            ))
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
        InitSubmodule => {
            let (path, oid) = input.split_once('|').unwrap_or((input, ""));
            let plan = submodules::prepare_init_with_credentials(
                root,
                path.trim().as_bytes(),
                oid.trim(),
                credential,
                limits,
                cancelled,
            )?;
            let preview = plan.preview();
            let rows = vec![
                format!("Ruta · {}", safe_label(&preview.path)),
                format!("Origen · {}", safe_label(&preview.source)),
                format!("Commit · {}", safe_label(&preview.commit)),
                format!("{} archivos · {} bytes", preview.files, preview.bytes),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::InitSubmodule(Box::new(plan)),
            ))
        }
        UpdateSubmodule => {
            let (path, target) = input.split_once('|').unwrap_or((input, ""));
            let plan = submodules::prepare_update_with_credentials(
                root,
                path.trim().as_bytes(),
                "",
                target.trim(),
                credential,
                limits,
                cancelled,
            )?;
            let preview = plan.preview();
            let rows = vec![
                format!("Ruta · {}", safe_label(&preview.path)),
                format!("Origen · {}", safe_label(&preview.source)),
                format!(
                    "{} → {}",
                    safe_label(&preview.previous_commit),
                    safe_label(&preview.target_commit)
                ),
                format!("{} archivos · {} bytes", preview.files, preview.bytes),
            ];
            Ok(RepositoryReview::mutation(
                rows,
                Publication::UpdateSubmodule(Box::new(plan)),
            ))
        }
        RecoverSubmodule => {
            let mut cancelled = cancelled;
            let (rows, recovery) =
                match submodules::preview_init_recovery(root, limits, &mut cancelled) {
                    Ok(Some(preview)) => (
                        vec![
                            format!("Inicialización · {}", safe_label(&preview.path)),
                            preview.commit.clone(),
                        ],
                        SubmoduleRecovery::Init(preview),
                    ),
                    Ok(None) => {
                        return Ok(RepositoryReview::inspection(vec![
                            "No hay una operación pendiente".into(),
                        ]))
                    }
                    Err(Error::Unsupported) => {
                        let preview = submodules::preview_update_recovery(root, limits, cancelled)?
                            .ok_or(Error::Stale)?;
                        let rows = vec![
                            format!("Actualización · {}", safe_label(&preview.path)),
                            format!(
                                "{} → {}",
                                safe_label(&preview.previous_commit),
                                safe_label(&preview.target_commit)
                            ),
                        ];
                        (rows, SubmoduleRecovery::Update(preview))
                    }
                    Err(error) => return Err(error),
                };
            Ok(RepositoryReview::mutation(
                rows,
                Publication::RecoverSubmodule(root.to_owned(), recovery, limits),
            ))
        }
    }
}

fn prepare_pull(
    root: &Path,
    requested: &str,
    credential: Option<&remote::OperationCredential>,
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
    let fetch = prepare_fetch(root, &remote.name, credential, limits, cancelled)?;
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

fn prepare_fetch(
    root: &Path,
    name: &str,
    credential: Option<&remote::OperationCredential>,
    limits: Limits,
    mut cancelled: impl FnMut() -> bool + Send,
) -> Result<remote::FetchPlan, Error> {
    if remote::configured_remote_is_network(root, name, false, limits, &mut cancelled)? {
        remote::prepare_network_fetch(root, name, credential, limits, cancelled)
    } else {
        remote::prepare_configured_fetch(root, name, limits, cancelled)
    }
}

fn apply_fetch(
    plan: remote::FetchPlan,
    credential: Option<&remote::OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<remote::TransferResult, Error> {
    if remote::fetch_preview(&plan).file_transport {
        remote::apply_local_fetch(plan, cancelled)
    } else {
        remote::apply_network_fetch(plan, credential, cancelled)
    }
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

fn worktree_move_rows(id: &str, source: &Path, destination: &Path) -> Vec<String> {
    vec![
        format!("Árbol · {}", safe_label(id)),
        format!("Origen · {}", safe_label(&source.to_string_lossy())),
        format!("Destino · {}", safe_label(&destination.to_string_lossy())),
    ]
}

fn worktree_moved(path: &Path) -> String {
    format!("Árbol trasladado · {}", safe_label(&path.to_string_lossy()))
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
        CloneWorktree,
        AddRemote,
        RemoveRemote,
        Upstream,
        Worktrees,
        CreateWorktree,
        ResumeWorktree,
        MoveWorktree,
        ResumeMoveWorktree,
        RetireWorktree,
        RestoreRetiredWorktree,
        LockWorktree,
        UnlockWorktree,
        Submodules,
        InitSubmodule,
        UpdateSubmodule,
        RecoverSubmodule,
    ]
    .into_iter()
    .filter(move |action| {
        (stash || !action.uses_stash())
            && (cfg!(windows) || matches!(action, Remotes | Fetch | Push | Worktrees | Submodules))
    })
}
