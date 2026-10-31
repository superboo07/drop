use std::{
    fs::{self, metadata},
    path::{Path, PathBuf},
};

// Symbolic links are collected as entries of their own and never followed,
// matching `versions::_list_files` -- see the note there.
fn list_files_recursive(vec: &mut Vec<PathBuf>, path: &Path) {
    if metadata(path).unwrap().is_dir() {
        let paths = fs::read_dir(path).unwrap();
        for path_result in paths {
            let entry = path_result.unwrap();
            let full_path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                list_files_recursive(vec, &full_path);
            } else {
                vec.push(full_path);
            }
        }
    }
}

pub fn list_files(path: &Path) -> Vec<PathBuf> {
    let mut vec = Vec::new();
    list_files_recursive(&mut vec, path);
    vec
}
