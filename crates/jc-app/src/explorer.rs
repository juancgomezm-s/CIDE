//! Lazy project directory catalog, shared by frontends. It owns metadata only.
mod view;
mod worker;
use crate::{Cancellation, TabId, Wakeup, Workbench};
use jc_text::TextEncoding;
use std::{
    mem::size_of,
    path::{Component, Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    thread::JoinHandle,
    time::Duration,
};
pub use view::{ExplorerPresentation, ExplorerRow, ExplorerView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    generation: u64,
    ordinal: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerLoadState {
    Unloaded,
    Queued,
    Loading,
    Ready,
    Limited,
    Cancelled,
    Failed,
}
#[derive(Debug)]
pub struct ExplorerNode {
    id: NodeId,
    parent: Option<NodeId>,
    relative: PathBuf,
    is_dir: bool,
    expanded: bool,
    state: ExplorerLoadState,
    directory_revision: u64,
    depth: usize,
    children: Vec<NodeId>,
}
impl ExplorerNode {
    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
    pub fn relative_path(&self) -> &Path {
        &self.relative
    }
    pub fn expanded(&self) -> bool {
        self.expanded
    }
    pub fn state(&self) -> ExplorerLoadState {
        self.state
    }
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ExplorerLimits {
    pub max_nodes: usize,
    pub max_owned_bytes: usize,
    pub max_pending: usize,
    pub max_pending_bytes: usize,
    pub max_directory_entries: usize,
    pub max_directory_bytes: usize,
    pub max_depth: usize,
    pub max_expanded: usize,
    pub timeout: Duration,
}
impl Default for ExplorerLimits {
    fn default() -> Self {
        Self {
            max_nodes: 4096,
            max_owned_bytes: 1024 * 1024,
            max_pending: 8,
            max_pending_bytes: 128 * 1024,
            max_directory_entries: 2048,
            max_directory_bytes: 512 * 1024,
            max_depth: 32,
            max_expanded: 32,
            timeout: Duration::from_secs(2),
        }
    }
}
impl ExplorerLimits {
    pub fn valid(self) -> bool {
        (1..=4096).contains(&self.max_nodes)
            && (size_of::<ExplorerNode>()..=1024 * 1024).contains(&self.max_owned_bytes)
            && (1..=8).contains(&self.max_pending)
            && (1024..=128 * 1024).contains(&self.max_pending_bytes)
            && (1..=2048).contains(&self.max_directory_entries)
            && (4096..=512 * 1024).contains(&self.max_directory_bytes)
            && (1..=32).contains(&self.max_depth)
            && (1..=32).contains(&self.max_expanded)
            && !self.timeout.is_zero()
            && self.timeout <= Duration::from_secs(2)
    }
}
fn path_units(path: &Path) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count()
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().len()
    }
}
fn shift_index(current: usize, delta: isize, max: usize) -> usize {
    if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize)
    }
    .min(max)
}
pub struct ProjectExplorer {
    shared: Arc<worker::Shared>,
    worker: Option<JoinHandle<()>>,
    limits: ExplorerLimits,
    root: Option<PathBuf>,
    nodes: Vec<ExplorerNode>,
    generation: u64,
    revision: u64,
    ordinal: u64,
    message: &'static str,
    examined: usize,
    skipped: usize,
}
impl ProjectExplorer {
    pub fn new(wakeup: Wakeup, limits: ExplorerLimits) -> Result<Self, String> {
        if !limits.valid() {
            return Err("Límites del explorador inválidos".into());
        }
        Ok(Self {
            shared: Arc::new(worker::Shared::new(wakeup)),
            worker: None,
            limits,
            root: None,
            nodes: Vec::new(),
            generation: 0,
            revision: 0,
            ordinal: 0,
            message: "Elige una carpeta de proyecto",
            examined: 0,
            skipped: 0,
        })
    }
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
    pub fn root_id(&self) -> Option<NodeId> {
        self.nodes.first().map(|n| n.id)
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn node(&self, id: NodeId) -> Option<&ExplorerNode> {
        if id.generation != self.generation {
            return None;
        }
        self.nodes.iter().find(|n| n.id == id)
    }
    /// Native path for a live catalog identity. Display labels are never parsed
    /// back into paths; generation rejection also covers a changed browser root.
    pub fn path(&self, id: NodeId) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join(&self.node(id)?.relative))
    }
    pub fn nodes(&self) -> &[ExplorerNode] {
        &self.nodes
    }
    pub fn owned_bytes(&self) -> usize {
        self.root.as_ref().map_or(0, PathBuf::capacity)
            + self.nodes.capacity() * size_of::<ExplorerNode>()
            + self
                .nodes
                .iter()
                .map(|n| n.relative.capacity() + n.children.capacity() * size_of::<NodeId>())
                .sum::<usize>()
    }
    pub fn visible_nodes(&self) -> impl Iterator<Item = &ExplorerNode> {
        let mut hidden: Option<usize> = None;
        self.nodes.iter().filter(move |node| {
            if hidden.is_some_and(|depth| node.depth > depth) {
                return false;
            }
            hidden = if node.is_dir && !node.expanded {
                Some(node.depth)
            } else {
                None
            };
            true
        })
    }
    fn bump(&mut self) {
        if let Some(next) = self.revision.checked_add(1) {
            self.revision = next;
        } else {
            self.stop_all();
            self.root = None;
            self.nodes = Vec::new();
            self.message = "Se agotó la revisión del explorador";
        }
    }
    fn mutable(&self) -> Result<(), String> {
        if self.revision == u64::MAX {
            Err("Se agotó la revisión del explorador".into())
        } else {
            Ok(())
        }
    }
    fn index(&self, id: NodeId) -> Result<usize, String> {
        self.nodes
            .iter()
            .position(|n| n.id == id && id.generation == self.generation)
            .ok_or_else(|| "El nodo del explorador está obsoleto".into())
    }
    fn stop_all(&self) {
        let mut q = self.shared.lock();
        if let Some(active) = &q.active {
            active.cancel.cancel();
        }
        for request in &q.pending {
            request.cancel.cancel();
        }
        q.pending.clear();
        q.completion = None;
        q.generation = None;
        drop(q);
        self.shared.changed.notify_all();
    }
    pub fn clear(&mut self) {
        self.stop_all();
        self.generation = self.generation.saturating_add(1);
        self.root = None;
        self.nodes = Vec::new();
        self.ordinal = 0;
        self.message = "Elige una carpeta de proyecto";
        self.examined = 0;
        self.skipped = 0;
        self.bump();
    }
    pub fn set_root(&mut self, root: PathBuf) -> Result<(), String> {
        self.mutable()?;
        if !root.is_absolute()
            || path_units(&root) > 4096
            || root
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("El explorador requiere una raíz absoluta admitida".into());
        }
        if root.capacity().saturating_add(size_of::<ExplorerNode>()) > self.limits.max_owned_bytes {
            return Err("La raíz del explorador excede la cuota del catálogo".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Se agotó la generación del explorador")?;
        self.stop_all();
        self.generation = generation;
        self.ordinal = 1;
        self.root = Some(root);
        self.nodes = Vec::new();
        self.nodes.reserve_exact(1);
        let id = NodeId {
            generation,
            ordinal: 1,
        };
        self.nodes.push(ExplorerNode {
            id,
            parent: None,
            relative: PathBuf::new(),
            is_dir: true,
            expanded: false,
            state: ExplorerLoadState::Unloaded,
            directory_revision: 0,
            depth: 0,
            children: Vec::new(),
        });
        self.examined = 0;
        self.skipped = 0;
        self.shared.lock().generation = Some(generation);
        self.bump();
        self.expand(id)
    }
    fn ensure_worker(&mut self) -> Result<(), String> {
        self.recover_worker();
        if self.worker.is_none() {
            let shared = self.shared.clone();
            self.worker = Some(
                std::thread::Builder::new()
                    .name("cide-explorer".into())
                    .spawn(move || worker::run(shared))
                    .map_err(|_| "No se pudo iniciar la tarea de metadatos del explorador")?,
            );
        }
        Ok(())
    }
    fn recover_worker(&mut self) -> bool {
        if !self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            return false;
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        {
            let mut q = self.shared.lock();
            if let Some(a) = q.active.take() {
                a.cancel.cancel();
            }
            q.pending.clear();
            q.completion = None;
        }
        for node in &mut self.nodes {
            if matches!(
                node.state,
                ExplorerLoadState::Queued | ExplorerLoadState::Loading
            ) {
                node.state = ExplorerLoadState::Failed;
            }
        }
        self.message = "Se detuvo la tarea del explorador; actualiza para reintentar";
        self.shared.recovery.store(true, Ordering::Release);
        self.bump();
        true
    }
    fn enqueue(&mut self, index: usize) -> Result<(), String> {
        if self.ordinal == u64::MAX {
            return Err("Se agotó la identidad de nodos; elige de nuevo la raíz".into());
        }
        let root = self.root.as_ref().ok_or("El explorador no tiene raíz")?;
        let node = &self.nodes[index];
        if node.depth >= self.limits.max_depth {
            return Err("Se alcanzó el límite de profundidad del explorador".into());
        }
        let directory_revision = node
            .directory_revision
            .checked_add(1)
            .ok_or("Se agotó la revisión del directorio")?;
        let request = worker::Request {
            generation: self.generation,
            node: node.id,
            directory_revision,
            root: root.clone(),
            relative: node.relative.clone(),
            limits: self.limits,
            cancel: Cancellation::new(self.limits.timeout),
        };
        if path_units(&request.root.join(&request.relative)) > 4096 {
            return Err("La ruta del directorio excede el límite".into());
        }
        {
            let q = self.shared.lock();
            let bytes = q
                .pending
                .iter()
                .filter(|r| r.node != request.node)
                .map(|r| r.charge() - size_of::<worker::Request>())
                .sum::<usize>();
            let count = q.pending.iter().filter(|r| r.node != request.node).count();
            if count >= self.limits.max_pending
                || q.pending.capacity().max(count + 1) * size_of::<worker::Request>()
                    + bytes
                    + request.charge()
                    - size_of::<worker::Request>()
                    > self.limits.max_pending_bytes
            {
                return Err("El explorador está ocupado; reintenta la expansión".into());
            }
        }
        self.ensure_worker()?;
        {
            let mut q = self.shared.lock();
            let count = q.pending.iter().filter(|r| r.node != request.node).count();
            let additional = (count + 1).saturating_sub(q.pending.len());
            q.pending.reserve_exact(additional);
            let paths = q
                .pending
                .iter()
                .filter(|r| r.node != request.node)
                .map(|r| r.charge() - size_of::<worker::Request>())
                .sum::<usize>()
                + request.charge()
                - size_of::<worker::Request>();
            if count >= self.limits.max_pending
                || q.pending.capacity() * size_of::<worker::Request>() + paths
                    > self.limits.max_pending_bytes
            {
                q.pending.shrink_to_fit();
                return Err("El explorador está ocupado; reintenta la expansión".into());
            }
            q.pending.retain(|old| {
                if old.node == request.node {
                    old.cancel.cancel();
                    false
                } else {
                    true
                }
            });
            if let Some(a) = &q.active {
                if a.node == request.node {
                    a.cancel.cancel();
                }
            }
            if q.completion
                .as_ref()
                .is_some_and(|c| c.node == request.node)
            {
                q.completion = None;
            }
            q.pending.push_back(request);
        }
        self.nodes[index].directory_revision = directory_revision;
        self.nodes[index].state = ExplorerLoadState::Queued;
        self.nodes[index].expanded = true;
        self.message = "Leyendo metadatos del directorio";
        self.bump();
        self.shared.changed.notify_all();
        Ok(())
    }
    pub fn expand(&mut self, id: NodeId) -> Result<(), String> {
        self.mutable()?;
        let index = self.index(id)?;
        if !self.nodes[index].is_dir {
            return Err("El nodo del explorador no es un directorio".into());
        }
        if !self.nodes[index].expanded
            && self.nodes.iter().filter(|n| n.expanded).count() >= self.limits.max_expanded
        {
            return Err("Se alcanzó el límite de directorios expandidos".into());
        }
        if matches!(
            self.nodes[index].state,
            ExplorerLoadState::Ready
                | ExplorerLoadState::Limited
                | ExplorerLoadState::Queued
                | ExplorerLoadState::Loading
        ) {
            self.nodes[index].expanded = true;
            self.bump();
            return Ok(());
        }
        self.enqueue(index)
    }
    fn subtree_end(&self, index: usize) -> usize {
        let depth = self.nodes[index].depth;
        index
            + 1
            + self.nodes[index + 1..]
                .iter()
                .take_while(|n| n.depth > depth)
                .count()
    }
    fn cancel_ids(&self, ids: &[NodeId]) {
        let mut q = self.shared.lock();
        if let Some(a) = &q.active {
            if ids.contains(&a.node) {
                a.cancel.cancel();
            }
        }
        q.pending.retain(|r| {
            if ids.contains(&r.node) {
                r.cancel.cancel();
                false
            } else {
                true
            }
        });
        if q.completion.as_ref().is_some_and(|c| ids.contains(&c.node)) {
            q.completion = None;
        }
        drop(q);
        self.shared.changed.notify_all();
    }
    fn evict_children(&mut self, index: usize) {
        let end = self.subtree_end(index);
        let ids: Vec<_> = self.nodes[index + 1..end].iter().map(|n| n.id).collect();
        self.cancel_ids(&ids);
        self.nodes.drain(index + 1..end);
        self.nodes[index].children = Vec::new();
        // Reclaim header capacity after a large collapse without shrinking on
        // every small interaction. Paths of evicted nodes drop immediately.
        if self.nodes.capacity().saturating_sub(self.nodes.len()) * size_of::<ExplorerNode>()
            > 32 * 1024
            && self.nodes.capacity() > self.nodes.len().saturating_mul(2)
        {
            self.nodes.shrink_to_fit();
        }
    }
    pub fn cancel(&mut self, id: NodeId) -> Result<(), String> {
        self.mutable()?;
        let index = self.index(id)?;
        if !self.nodes[index].is_dir {
            return Err("El nodo del explorador no es un directorio".into());
        }
        let revision = self.nodes[index]
            .directory_revision
            .checked_add(1)
            .ok_or("Se agotó la revisión del directorio")?;
        let end = self.subtree_end(index);
        let ids: Vec<_> = self.nodes[index..end].iter().map(|n| n.id).collect();
        self.cancel_ids(&ids);
        for node in &mut self.nodes[index..end] {
            if matches!(
                node.state,
                ExplorerLoadState::Queued | ExplorerLoadState::Loading
            ) {
                node.state = ExplorerLoadState::Cancelled;
            }
        }
        self.nodes[index].directory_revision = revision;
        self.nodes[index].state = ExplorerLoadState::Cancelled;
        self.message = "Carga del directorio cancelada; actualiza para reintentar";
        self.bump();
        Ok(())
    }
    pub fn collapse(&mut self, id: NodeId) -> Result<(), String> {
        self.cancel(id)?;
        let index = self.index(id)?;
        self.evict_children(index);
        self.nodes[index].expanded = false;
        self.nodes[index].state = ExplorerLoadState::Unloaded;
        self.bump();
        Ok(())
    }
    pub fn refresh(&mut self, id: NodeId) -> Result<(), String> {
        self.mutable()?;
        let index = self.index(id)?;
        if !self.nodes[index].is_dir {
            return Err("Actualizar requiere un identificador de directorio".into());
        }
        // Busy leaves existing children and the old view unchanged.
        self.enqueue(index)?;
        self.evict_children(index);
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.shared.lock().completion.is_some()
    }
    pub fn take_recovery(&self) -> bool {
        self.shared.recovery.swap(false, Ordering::AcqRel)
    }
    pub fn poll(&mut self, current_root: &Path, max_deliveries: usize) -> bool {
        if self
            .root
            .as_deref()
            .is_some_and(|root| root != current_root)
        {
            self.clear();
            return true;
        }
        let mut changed = self.recover_worker();
        let active = {
            let q = self.shared.lock();
            q.active.as_ref().map(|a| (a.node, a.directory_revision))
        };
        if let Some((id, rev)) = active {
            if let Ok(index) = self.index(id) {
                if self.nodes[index].directory_revision == rev
                    && self.nodes[index].state == ExplorerLoadState::Queued
                {
                    self.nodes[index].state = ExplorerLoadState::Loading;
                    changed = true;
                    self.bump();
                }
            }
        }
        for _ in 0..max_deliveries.min(2) {
            let completion = self.shared.lock().completion.take();
            let Some(completion) = completion else {
                break;
            };
            self.shared.changed.notify_all();
            if completion.generation != self.generation
                || self.root.as_deref() != Some(completion.root.as_path())
            {
                continue;
            }
            let Ok(index) = self.index(completion.node) else {
                continue;
            };
            if self.nodes[index].directory_revision != completion.directory_revision
                || !self.nodes[index].expanded
                || !matches!(
                    self.nodes[index].state,
                    ExplorerLoadState::Queued | ExplorerLoadState::Loading
                )
            {
                continue;
            }
            changed = true;
            match completion.result {
                Ok(listing) => self.accept(index, listing),
                Err(message) => {
                    self.nodes[index].state = if completion.cancelled {
                        ExplorerLoadState::Cancelled
                    } else {
                        ExplorerLoadState::Failed
                    };
                    self.message = message;
                }
            }
            self.bump();
        }
        changed
    }
    fn accept(&mut self, index: usize, listing: jc_files::DirectoryListing) {
        self.examined = self.examined.saturating_add(listing.examined);
        self.skipped = self.skipped.saturating_add(listing.skipped);
        self.evict_children(index);
        let parent = self.nodes[index].id;
        let relative = self.nodes[index].relative.clone();
        let depth = self.nodes[index].depth + 1;
        let existing_paths = self
            .nodes
            .iter()
            .map(|n| n.relative.capacity() + n.children.capacity() * size_of::<NodeId>())
            .sum::<usize>();
        let root_charge = self.root.as_ref().map_or(0, PathBuf::capacity);
        let mut children = Vec::new();
        let staged = listing
            .entries
            .len()
            .min(self.limits.max_nodes.saturating_sub(self.nodes.len()))
            .min(
                self.limits.max_directory_bytes / (size_of::<ExplorerNode>() + size_of::<NodeId>()),
            );
        children.reserve_exact(staged);
        let mut paths = 0usize;
        let mut limited = listing.limited;
        let mut identity_exhausted = false;
        for entry in listing.entries {
            let name = Path::new(&entry.name);
            if name.components().count() != 1
                || !matches!(name.components().next(), Some(Component::Normal(_)))
            {
                limited = true;
                self.skipped = self.skipped.saturating_add(1);
                continue;
            }
            let path = relative.join(name);
            if path_units(&self.root.as_ref().unwrap().join(&path)) > 4096 {
                limited = true;
                self.skipped = self.skipped.saturating_add(1);
                continue;
            }
            let needed = self.nodes.len() + children.len() + 1;
            let charge = root_charge
                + self.nodes.capacity().max(needed) * size_of::<ExplorerNode>()
                + existing_paths
                + paths
                + path.capacity()
                + (children.len() + 1) * size_of::<NodeId>();
            let staging = children.capacity() * size_of::<ExplorerNode>()
                + paths
                + path.capacity()
                + (children.len() + 1) * size_of::<NodeId>();
            if children.len() == staged
                || needed > self.limits.max_nodes
                || charge > self.limits.max_owned_bytes
                || staging > self.limits.max_directory_bytes
            {
                limited = true;
                break;
            }
            let Some(ordinal) = self.ordinal.checked_add(1) else {
                identity_exhausted = true;
                break;
            };
            self.ordinal = ordinal;
            let id = NodeId {
                generation: self.generation,
                ordinal,
            };
            paths += path.capacity();
            children.push(ExplorerNode {
                id,
                parent: Some(parent),
                relative: path,
                is_dir: entry.is_dir,
                expanded: false,
                state: if entry.is_dir {
                    ExplorerLoadState::Unloaded
                } else {
                    ExplorerLoadState::Ready
                },
                directory_revision: 0,
                depth,
                children: Vec::new(),
            });
        }
        self.nodes.reserve_exact(children.len());
        let ids: Vec<_> = children.iter().map(|n| n.id).collect();
        if root_charge
            + self.nodes.capacity() * size_of::<ExplorerNode>()
            + existing_paths
            + paths
            + ids.capacity() * size_of::<NodeId>()
            > self.limits.max_owned_bytes
        {
            children.clear();
            limited = true;
            self.nodes.shrink_to_fit();
        } else {
            self.nodes[index].children = ids;
            self.nodes.splice(index + 1..index + 1, children);
        }
        self.nodes[index].state = if identity_exhausted {
            ExplorerLoadState::Failed
        } else if limited {
            ExplorerLoadState::Limited
        } else {
            ExplorerLoadState::Ready
        };
        self.message = if identity_exhausted {
            "Se agotó la identidad de nodos; elige de nuevo la raíz"
        } else if limited {
            "Metadatos del directorio limitados; actualiza para reintentar"
        } else {
            "Metadatos del directorio listos"
        };
    }
    pub fn open_file(
        &self,
        id: NodeId,
        app: &mut Workbench,
        encoding: Option<TextEncoding>,
        to_side: bool,
        max_bytes: usize,
    ) -> Result<TabId, String> {
        let (root, path, _verified) = self.resolved_file_open(id)?;
        app.open_project_file(root, path, encoding, to_side, max_bytes)
            .map_err(|e| e.to_string())
    }

    pub fn request_file_open(
        &self,
        id: NodeId,
        app: &mut Workbench,
        encoding: Option<TextEncoding>,
        to_side: bool,
        resident_limit: usize,
    ) -> Result<Option<TabId>, String> {
        let (root, path, _verified) = self.resolved_file_open(id)?;
        app.request_file_open_in_root(root, path, encoding, to_side, resident_limit)
            .map_err(|e| e.to_string())
    }

    fn resolved_file_open(&self, id: NodeId) -> Result<(&Path, PathBuf, std::fs::File), String> {
        let node = self
            .node(id)
            .ok_or("El nodo del explorador está obsoleto")?;
        if node.is_dir {
            return Err("Un directorio del explorador no puede abrirse como archivo".into());
        }
        let root = self.root.as_ref().ok_or("El explorador no tiene raíz")?;
        let path = root.join(&node.relative);
        // An already-open target takes Workbench's deduplication route. Check
        // the current native leaf/chain first so a replacement link cannot
        // redirect that route to some other dirty document inside the root.
        let verified = verified_open_target(root, &path)?;
        Ok((root, path, verified))
    }
}

fn reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn ordinary_chain(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "El destino del explorador está fuera de la raíz del proyecto")?;
    let root_metadata =
        std::fs::symlink_metadata(root).map_err(|_| "La raíz del explorador no está disponible")?;
    if !root_metadata.is_dir() || reparse(&root_metadata) {
        return Err("La raíz del explorador no es un directorio ordinario".into());
    }
    let mut current = root.to_owned();
    let mut components = relative.components().peekable();
    if components.peek().is_none() {
        return Err("El destino del explorador no es un archivo".into());
    }
    while let Some(component) = components.next() {
        if !matches!(component, Component::Normal(_)) {
            return Err("El destino del explorador contiene componentes inválidos".into());
        }
        current.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| "El destino del explorador no está disponible")?;
        if reparse(&metadata)
            || if components.peek().is_some() {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            }
        {
            return Err(
                "El destino contiene un enlace/punto de reanálisis o cambió de tipo".into(),
            );
        }
    }
    Ok(())
}
fn verified_open_target(root: &Path, path: &Path) -> Result<std::fs::File, String> {
    ordinary_chain(root, path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the leaf itself, not a replacement reparse target, and retain
        // the no-delete-share guard through the Workbench activation boundary.
        options.custom_flags(0x0020_0000).share_mode(3);
    }
    let file = options
        .open(path)
        .map_err(|_| "El destino del explorador no puede abrirse de forma segura")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Los metadatos del destino no están disponibles")?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err("El destino del explorador no es un archivo ordinario".into());
    }
    jc_files::verify_file_in_root(root, &file)
        .map_err(|_| "El destino abierto salió de la raíz del proyecto")?;
    ordinary_chain(root, path)?;
    Ok(file)
}
impl Drop for ProjectExplorer {
    fn drop(&mut self) {
        self.stop_all();
        {
            let mut q = self.shared.lock();
            q.stop = true;
        }
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod admission_regressions {
    use super::*;
    use jc_files::{DirectoryEntry, DirectoryListing};

    #[test]
    fn cancelled_descendant_cannot_accept_a_late_success_even_with_matching_revision() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("virtual-cancel-admission");
        let mut catalog = ProjectExplorer::new(Wakeup::default(), Default::default()).unwrap();
        catalog.root = Some(root.clone());
        catalog.generation = 1;
        catalog.ordinal = 2;
        let parent = NodeId {
            generation: 1,
            ordinal: 1,
        };
        let child = NodeId {
            generation: 1,
            ordinal: 2,
        };
        catalog.nodes = vec![
            ExplorerNode {
                id: parent,
                parent: None,
                relative: PathBuf::new(),
                is_dir: true,
                expanded: true,
                state: ExplorerLoadState::Ready,
                directory_revision: 1,
                depth: 0,
                children: vec![child],
            },
            ExplorerNode {
                id: child,
                parent: Some(parent),
                relative: "folder".into(),
                is_dir: true,
                expanded: true,
                state: ExplorerLoadState::Loading,
                directory_revision: 3,
                depth: 1,
                children: Vec::new(),
            },
        ];
        catalog.cancel(parent).unwrap();
        let revision = catalog.revision();
        // Reproduce a provider that completed before observing the token but
        // publishes after cancel. Parent cancellation does not alter child rev3.
        catalog.shared.lock().completion = Some(worker::Completion {
            generation: 1,
            node: child,
            directory_revision: 3,
            root: root.clone(),
            cancelled: false,
            result: Ok(DirectoryListing {
                entries: vec![DirectoryEntry {
                    name: "late.txt".into(),
                    is_dir: false,
                }],
                limited: false,
                examined: 1,
                skipped: 0,
            }),
        });
        assert!(!catalog.poll(&root, 2));
        assert_eq!(catalog.revision(), revision);
        assert_eq!(
            catalog.node(child).unwrap().state(),
            ExplorerLoadState::Cancelled
        );
        assert!(catalog.node(child).unwrap().children().is_empty());
        assert_eq!(catalog.nodes.len(), 2);
    }

    struct Fixture {
        root: PathBuf,
        path: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            use std::{
                io::Write,
                sync::atomic::{AtomicU64, Ordering},
                time::{SystemTime, UNIX_EPOCH},
            };
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let mut base = workspace.clone();
            for name in ["work", "test-temp"] {
                base.push(name);
                match std::fs::create_dir(&base) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => panic!("fixture directory: {e}"),
                }
                let m = std::fs::symlink_metadata(&base).unwrap();
                assert!(m.is_dir() && !reparse(&m));
                base = base.canonicalize().unwrap();
                assert!(base.starts_with(&workspace));
            }
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = base.join(format!(
                "explorer-open-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let root = root.canonicalize().unwrap();
            let path = root.join("source.txt");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(b"alpha\r\n").unwrap();
            Self { root, path }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.root.canonicalize().ok().as_ref() != Some(&self.root) {
                return;
            }
            let Ok(root) = std::fs::symlink_metadata(&self.root) else {
                return;
            };
            let Ok(leaf) = std::fs::symlink_metadata(&self.path) else {
                return;
            };
            if reparse(&root)
                || reparse(&leaf)
                || self
                    .path
                    .canonicalize()
                    .ok()
                    .and_then(|p| p.parent().map(Path::to_owned))
                    != Some(self.root.clone())
            {
                return;
            }
            if leaf.is_file() {
                let _ = std::fs::remove_file(&self.path);
            } else if leaf.is_dir() {
                let _ = std::fs::remove_dir(&self.path);
            }
            let _ = std::fs::remove_dir(&self.root);
        }
    }
    #[test]
    fn explicit_open_reuses_dirty_document_but_rejects_changed_leaf_before_focus() {
        let fixture = Fixture::new();
        let mut catalog = ProjectExplorer::new(Wakeup::default(), Default::default()).unwrap();
        catalog.root = Some(fixture.root.clone());
        catalog.generation = 1;
        catalog.ordinal = 2;
        let parent = NodeId {
            generation: 1,
            ordinal: 1,
        };
        let id = NodeId {
            generation: 1,
            ordinal: 2,
        };
        catalog.nodes.push(ExplorerNode {
            id: parent,
            parent: None,
            relative: PathBuf::new(),
            is_dir: true,
            expanded: true,
            state: ExplorerLoadState::Ready,
            directory_revision: 1,
            depth: 0,
            children: vec![id],
        });
        catalog.nodes.push(ExplorerNode {
            id,
            parent: Some(parent),
            relative: "source.txt".into(),
            is_dir: false,
            expanded: false,
            state: ExplorerLoadState::Ready,
            directory_revision: 0,
            depth: 1,
            children: Vec::new(),
        });
        let mut app = Workbench::default();
        let tab = app
            .open_file(fixture.path.clone(), Some(TextEncoding::Utf8))
            .unwrap();
        app.insert_text("Q").unwrap();
        let document = app.active_document().unwrap().id;
        let other = app.new_file(fixture.root.join("memory.txt"), TextEncoding::Utf8);
        app.insert_text("keep focus").unwrap();
        assert_eq!(
            catalog
                .open_file(id, &mut app, Some(TextEncoding::Utf8), false, 1024)
                .unwrap(),
            tab
        );
        assert_eq!(app.active_document().unwrap().id, document);
        assert_eq!(app.active_bytes().unwrap(), b"Qalpha\r\n");
        assert!(app.jump_to_location(other, 0));
        let before = format!("{:?}|{:?}", app.panes(), app.tabs());
        std::fs::remove_file(&fixture.path).unwrap();
        std::fs::create_dir(&fixture.path).unwrap();
        assert!(catalog
            .open_file(id, &mut app, Some(TextEncoding::Utf8), true, 1024)
            .is_err());
        assert_eq!(format!("{:?}|{:?}", app.panes(), app.tabs()), before);
        assert_eq!(
            app.document_by_id(document)
                .unwrap()
                .session
                .document()
                .to_bytes(),
            b"Qalpha\r\n"
        );
        assert!(app
            .document_by_id(document)
            .unwrap()
            .session
            .document()
            .can_undo());
    }
}
