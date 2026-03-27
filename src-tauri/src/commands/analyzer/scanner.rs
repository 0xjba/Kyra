use super::{DirNode, ScanProgress};

pub fn scan_directory<F>(
    _path: &str,
    _depth: usize,
    _on_progress: F,
) -> DirNode
where
    F: FnMut(&ScanProgress),
{
    DirNode {
        name: String::new(),
        path: String::new(),
        size: 0,
        is_dir: true,
        children: vec![],
    }
}
