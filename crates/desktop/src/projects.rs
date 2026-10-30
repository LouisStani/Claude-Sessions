//! Projects: a folder each, with a tab of its own. A session belongs to the project whose
//! folder holds its working directory. The list is kept in `projects.json`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub directory: PathBuf,
}

impl Project {
    /// Named after its folder.
    pub fn new(directory: PathBuf) -> Self {
        let name = directory.file_name().map_or_else(|| directory.display().to_string(), |name| name.to_string_lossy().into_owned());
        Self { name, directory }
    }
}

/// The project holding `directory`, the deepest one when projects are nested. Paths are
/// compared by component: `~/Dev/api2` is not in `~/Dev/api`.
pub fn owner<'a, T>(directory: &Path, projects: impl IntoIterator<Item = (T, &'a Project)>) -> Option<T> {
    projects
        .into_iter()
        .filter(|(_, project)| directory.starts_with(&project.directory))
        .max_by_key(|(_, project)| project.directory.components().count())
        .map(|(id, _)| id)
}

/// No file yet means no projects.
pub fn load(path: &Path) -> io::Result<Vec<Project>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| io::Error::other(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Writes to a hidden temporary file first, then renames: a reader never sees half a list.
pub fn save(path: &Path, projects: &[Project]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_file_name(".projects.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(projects).map_err(io::Error::other)?)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(directory: &str) -> Project {
        Project::new(PathBuf::from(directory))
    }

    fn owner_of(directory: &str, projects: &[Project]) -> Option<usize> {
        owner(Path::new(directory), projects.iter().enumerate())
    }

    #[test]
    fn a_project_is_named_after_its_folder() {
        assert_eq!(project("/home/u/Dev/api").name, "api");
    }

    #[test]
    fn a_session_belongs_to_the_project_holding_its_directory() {
        let projects = [project("/home/u/Dev/api"), project("/home/u/Dev/blog")];
        assert_eq!(owner_of("/home/u/Dev/api", &projects), Some(0));
        assert_eq!(owner_of("/home/u/Dev/blog/src/posts", &projects), Some(1));
        assert_eq!(owner_of("/home/u", &projects), None);
        assert_eq!(owner_of("/tmp", &projects), None);
    }

    #[test]
    fn a_folder_sharing_a_prefix_is_not_inside() {
        let projects = [project("/home/u/Dev/api")];
        assert_eq!(owner_of("/home/u/Dev/api2", &projects), None);
    }

    #[test]
    fn nested_projects_give_the_session_to_the_deepest() {
        let projects = [project("/home/u/Dev"), project("/home/u/Dev/api"), project("/home/u")];
        assert_eq!(owner_of("/home/u/Dev/api/src", &projects), Some(1));
        assert_eq!(owner_of("/home/u/Dev/blog", &projects), Some(0));
        assert_eq!(owner_of("/home/u/Music", &projects), Some(2));
    }

    fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("claude-sessions-projects-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn saved_projects_load_back_in_order() {
        let root = scratch("round-trip");
        let path = root.join("claude-sessions/projects.json");
        let projects = vec![project("/home/u/Dev/blog"), project("/home/u/Dev/api")];
        save(&path, &projects).unwrap();
        assert_eq!(load(&path).unwrap(), projects);
        assert!(!path.with_file_name(".projects.json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_file_means_no_projects() {
        let root = scratch("missing");
        assert_eq!(load(&root.join("projects.json")).unwrap(), []);
    }

    #[test]
    fn a_broken_file_is_an_error_naming_it() {
        let root = scratch("broken");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("projects.json");
        fs::write(&path, b"[{").unwrap();
        let error = load(&path).unwrap_err().to_string();
        assert!(error.contains("projects.json"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }
}
