/*
we need to figure out:
 1. which enum values in src/Enums.h correspond to GAMEMODE, MAPMODE, TELEPORTERMODE, GAMECOMPLETE, GAMECOMPLETE2
 2. what are the struct offsets of Game::{roomx, roomy, state, gamestate, frames} in src/Game.h

*/

use anyhow::{anyhow, Result};
use std::{env, path::Path, fs, collections::HashMap};
use std::process::Command;
use git2::Repository;

// path to local directory where we will be keeping VVVVVV source files
const LOCAL: &str = "repo_copy";

// get the the content of a file in a git tree as a String.
fn get_file_contents(repo: &Repository, tree: &git2::Tree, path: &str) -> Result<String> {
    Ok(String::from_utf8(
     tree.get_path(&Path::new(path))?.to_object(repo)?.peel_to_blob()?.content().to_vec()
    )?)
}

// "download" source file from VVVVVV repo revision and all the files it includes via #include "..."
fn download_file_and_includes(repo: &Repository, tree: &git2::Tree, filename: &str, downloaded: &mut HashMap<String, String>) -> Result<()> {
    assert!(!filename.contains("..")); // don't write files outside of LOCAL
    if filename == "SDL.h" {
        // e.g. revision 6c85fae339f83f032230eafaf8ee7742f03dbbac includes SDL.h as
        //    #include "SDL.h"
        return Ok(());
    }
    if downloaded.contains_key(filename) {
        // already done
        return Ok(());
    }
    
    let contents = get_file_contents(repo, tree, &format!("desktop_version/src/{filename}"))?;
    fs::write(format!("{LOCAL}/{filename}"), &contents)?;
    let mut includes = vec![];
    let mut i = 0;
    loop {
        i = match contents[i..].find("#include \"") {
            Some(n) => i + n + "#include \"".len(),
            None => break,
        };
        let end = i + contents[i..].find("\"").unwrap();//TODO:error handling
        includes.push(contents[i..end].to_string());
    }
    downloaded.insert(filename.into(), contents);
    for inc in includes {
        download_file_and_includes(repo, tree, &inc, downloaded)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let src_dir = env::args().nth(1).ok_or_else(|| {
        anyhow!("Please provide the path to the VVVVVV source tree as a command-line argument.")
    })?;
    let repo = Repository::open(src_dir)?;
    let head = repo.head()?.target().unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.push(head)?;
    
    fs::create_dir_all(LOCAL)?;
    fs::write(format!("{LOCAL}/get-info.cpp"), include_str!("get-info.cpp"))?;
    
    let mut prev_downloaded = HashMap::new();
    
    for commit_id in revwalk {
        let commit_id = commit_id?;
        let commit = repo.find_object(commit_id, None)?.peel_to_commit()?;
        let tree = commit.tree()?;
        let mut downloaded = HashMap::new();
        download_file_and_includes(&repo, &tree, "Game.h", &mut downloaded)?;
        download_file_and_includes(&repo, &tree, "Enums.h", &mut downloaded)?;
        // only bother compiling if Game.h/Enums.h/any included file has changed
        if downloaded != prev_downloaded {
            let compile_status = Command::new("c++")
                .arg("-Wno-invalid-offsetof") // oh i dont care if offsetof is "undefined beghavior" in this case who cares nerd
                .arg("-I/usr/include/SDL2")
                .arg("-ISDL2") // if your SDL2 is somewhere else you can link/copy it to the cwd
                .arg(&format!("{LOCAL}/get-info.cpp"))
                .args(&["-o", &format!("{LOCAL}/get-info.out")])
                .status()?;
            if !compile_status.success() {
                return Err(anyhow!("c++ was not successful: {compile_status}"));
            }
            let output = Command::new(&format!("./{LOCAL}/get-info.out"))
                .output()?;
            let stdout = output.stdout;
            print!("{}", String::from_utf8(stdout)?);
            
        }
        prev_downloaded = downloaded;
    }
    
    Ok(())
}
