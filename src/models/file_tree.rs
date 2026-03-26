/// Represents a node in the file tree
#[derive(Clone, Debug)]
pub enum FileTreeNode {
    File {
        name: String,
        path: String,
    },
    Directory {
        name: String,
        path: String,
        children: Vec<FileTreeNode>,
        expanded: bool,
    },
}

impl FileTreeNode {
    pub fn name(&self) -> &str {
        match self {
            FileTreeNode::File { name, .. } => name,
            FileTreeNode::Directory { name, .. } => name,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            FileTreeNode::File { path, .. } => path,
            FileTreeNode::Directory { path, .. } => path,
        }
    }

    pub fn is_directory(&self) -> bool {
        matches!(self, FileTreeNode::Directory { .. })
    }
}

/// Build a file tree from a flat list of file paths.
/// Hides dotfiles/dotfolders (like .cache, .gitkeep) from the tree.
pub fn build_file_tree(paths: &[String]) -> Vec<FileTreeNode> {
    let mut root_children: Vec<FileTreeNode> = Vec::new();

    for path in paths {
        // Skip hidden files/folders (starting with .)
        if path.starts_with('.') || path.contains("/.") {
            continue;
        }
        let parts: Vec<&str> = path.split('/').collect();
        insert_into_tree(&mut root_children, &parts, path);
    }

    // Sort: directories first, then alphabetically
    sort_tree(&mut root_children);
    root_children
}

fn insert_into_tree(children: &mut Vec<FileTreeNode>, parts: &[&str], full_path: &str) {
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        // Leaf file
        let name = parts[0].to_string();
        if !children.iter().any(|c| c.name() == name && !c.is_directory()) {
            children.push(FileTreeNode::File {
                name,
                path: full_path.to_string(),
            });
        }
        return;
    }

    // Directory part
    let dir_name = parts[0];
    let dir_path = if full_path.contains('/') {
        let idx = full_path.find('/').unwrap_or(full_path.len());
        full_path[..idx].to_string()
    } else {
        dir_name.to_string()
    };

    // Find or create the directory node
    let dir_node = children.iter_mut().find(|c| c.name() == dir_name && c.is_directory());

    match dir_node {
        Some(FileTreeNode::Directory { children: dir_children, .. }) => {
            insert_into_tree(dir_children, &parts[1..], full_path);
        }
        _ => {
            let mut new_children = Vec::new();
            insert_into_tree(&mut new_children, &parts[1..], full_path);
            children.push(FileTreeNode::Directory {
                name: dir_name.to_string(),
                path: dir_path,
                children: new_children,
                expanded: true,
            });
        }
    }
}

fn sort_tree(nodes: &mut Vec<FileTreeNode>) {
    nodes.sort_by(|a, b| {
        match (a.is_directory(), b.is_directory()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name().to_lowercase().cmp(&b.name().to_lowercase()),
        }
    });

    for node in nodes.iter_mut() {
        if let FileTreeNode::Directory { children, .. } = node {
            sort_tree(children);
        }
    }
}
