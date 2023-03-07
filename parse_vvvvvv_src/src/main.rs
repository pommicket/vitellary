/*
we need to figure out:
 1. which enum values in src/Enums.h correspond to GAMEMODE, MAPMODE, TELEPORTERMODE, GAMECOMPLETE, GAMECOMPLETE2
 2. what are the struct offsets of Game::{roomx, roomy, state, gamestate, frames} in src/Game.h
*/

use anyhow::{anyhow, Result};
use git2::Repository;
use std::{
    collections::HashMap,
    env, fs,
    io::{self, prelude::*},
    path::Path,
    process::Command,
};

// path to local directory where we will be keeping VVVVVV source files
const LOCAL: &str = "repo_copy";

// get the the content of a file in a git tree as a String.
fn get_file_contents(repo: &Repository, tree: &git2::Tree, path: &str) -> Result<String> {
    Ok(String::from_utf8(
        tree.get_path(Path::new(path))?
            .to_object(repo)?
            .peel_to_blob()?
            .content()
            .to_vec(),
    )?)
}

// "download" source file from VVVVVV repo revision and all the files it includes via #include "..."
fn download_file_and_includes(
    repo: &Repository,
    tree: &git2::Tree,
    filename: &str,
    downloaded: &mut HashMap<String, String>,
) -> Result<()> {
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
        let end = i + contents[i..].find('"').expect("weird include");
        includes.push(contents[i..end].to_string());
    }
    downloaded.insert(filename.into(), contents);
    for inc in includes {
        download_file_and_includes(repo, tree, &inc, downloaded)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Revision {
    commit_id: String,
    game_size: u32,
    playing_states: Vec<u32>,
    offsets: HashMap<&'static str, u32>,
}

impl Revision {
    fn from_cache_contents(commit_id: String, contents: &str) -> Result<Self> {
        let mut lines = contents.lines();
        fn next_u32<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Result<u32> {
            lines
                .next()
                .ok_or_else(|| anyhow!("bad cache file: not enough lines"))?
                .parse::<u32>()
                .map_err(|e| anyhow::Error::from(e).context("bad cache file"))
        }
        let mut playing_states = vec![];
        for _ in 0..5 {
            playing_states.push(next_u32(&mut lines)?);
        }
        let game_size = next_u32(&mut lines)?;
        let fields = ["room_x", "room_y", "state", "gamestate", "timer"];
        let mut offsets = HashMap::new();
        for field in fields {
            offsets.insert(field, next_u32(&mut lines)?);
        }
        if lines.next() != Some(CACHE_IDENTIFIER.trim()) {
            return Err(anyhow!("bad cache file"));
        }
        Ok(Self {
            commit_id,
            playing_states,
            game_size,
            offsets,
        })
    }
}

// if we ever need to invalidate the cache (e.g. add more struct fields),
// we can change this string
const CACHE_IDENTIFIER: &str = "CACHE VERSION 2\n";

fn main() -> Result<()> {
    let src_dir = env::args().nth(1).ok_or_else(|| {
        anyhow!("Please provide the path to the VVVVVV source tree as a command-line argument.")
    })?;
    let repo = Repository::open(src_dir)?;
    let head = repo.head()?.target().unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.push(head)?;
    let commit_count = revwalk.count();
    revwalk = repo.revwalk()?;
    revwalk.push(head)?;

    fs::create_dir_all(LOCAL)?;
    fs::write(
        format!("{LOCAL}/get-info.cpp"),
        include_str!("get-info.cpp"),
    )?;

    let mut prev_downloaded = HashMap::new();

    let mut all_results = vec![];

    for (i, commit_id) in revwalk.enumerate() {
        let commit_id = commit_id?;
        let cache_path = format!("{LOCAL}/cache_{commit_id}");
        if let Ok(contents) = fs::read(&cache_path) {
            if let Ok(contents) = String::from_utf8(contents) {
                if contents.contains(CACHE_IDENTIFIER) {
                    all_results.push((commit_id, contents));
                    continue;
                }
            }
        }
        let commit = repo.find_object(commit_id, None)?.peel_to_commit()?;
        let tree = commit.tree()?;
        let mut downloaded = HashMap::new();
        download_file_and_includes(&repo, &tree, "Game.h", &mut downloaded)?;
        download_file_and_includes(&repo, &tree, "Enums.h", &mut downloaded)?;
        let mut results;
        // only bother compiling if Game.h/Enums.h/any included file has changed
        if downloaded != prev_downloaded {
            println!("Processing commit {commit_id} ({i}/{commit_count})...");
            let compile_status = Command::new("c++")
                .arg("-Wno-invalid-offsetof") // oh i dont care if offsetof is "undefined beghavior" in this case who cares nerd
                .arg("-I/usr/include/SDL2")
                .arg("-ISDL2") // if your SDL2 is somewhere else you can link/copy it to the cwd
                .arg(&format!("{LOCAL}/get-info.cpp"))
                .args(["-o", &format!("{LOCAL}/get-info.out")])
                .status()?;
            if !compile_status.success() {
                if i == 0 {
                    // the first compilation should succeed so something is going wrong with the C++ compiler
                    return Err(anyhow!("c++ was not successful: {compile_status}."));
                } else {
                    eprintln!("c++ was not successful: {compile_status}. ignoring this commit.");
                }
            }
            let output = Command::new(&format!("./{LOCAL}/get-info.out")).output()?;
            results = String::from_utf8(output.stdout)?;
            results.push_str(CACHE_IDENTIFIER);
        } else {
            // if nothing has changed, copy the previous resultsd
            results = all_results
                .last()
                .expect("the first commit should have new files")
                .1
                .clone();
        }
        // cache results
        fs::write(&cache_path, &results)?;
        all_results.push((commit_id, results));
        prev_downloaded = downloaded;
    }

    let revisions: Result<Vec<_>, _> = all_results
        .iter()
        .map(|(id, contents)| Revision::from_cache_contents(id.to_string(), contents))
        .collect();
    let mut revisions = revisions?;

    // this part here adds revisions for tagged commits and for the master branch's latest commit (refs/heads/master)
    for reference in repo.references()? {
        let reference = reference?;
        if let (Some(target), Some(name)) = (reference.target(), reference.name()) {
            if let Ok(target) = repo.find_object(target, None) {
                if let Ok(commit) = target.peel_to_commit() {
                    if name.starts_with("refs/tags/") || name == "refs/heads/master" {
                        let slash = name.rfind("/").unwrap();
                        let name = &name[slash + 1..];
                        let commit = commit.id().to_string();
                        if let Some(revision) = revisions.iter().find(|r| r.commit_id == commit) {
                            let mut revision = revision.clone();
                            revision.commit_id = name.to_string();
                            revisions.push(revision);
                        }
                    }
                }
            }
        }
    }

    let output_path = "../src/game/revisions.rs";
    let mut output = io::BufWriter::new(fs::File::create(output_path)?);
    writeln!(
        output,
        "// this file was auto-generated by parse_vvvvvv_src
use std::collections::HashMap;
use crate::Revision;\n"
    )?;
    writeln!(
        output,
        "pub(crate) fn get() -> HashMap<&'static str, Revision> {{ HashMap::from(["
    )?;
    for version in revisions {
        write!(output, "(\"{}\", Revision {{", version.commit_id)?;
        write!(output, "game_object_size: {},", version.game_size)?;
        // note: Debug output is not stable, so technically rust could change the Vec Debug implementation in the future.
        write!(
            output,
            "playing_states: vec![{}],",
            version
                .playing_states
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        // sort keys for consistent output
        let mut fields = version.offsets.keys().collect::<Vec<_>>();
        fields.sort();
        for field in fields {
            write!(output, "{field}_offset: {},", version.offsets[field])?;
        }
        writeln!(output, "}}),")?;
    }
    write!(output, "])}}")?;
    drop(output); // make sure file is sync'd

    let fmt_status = Command::new("rustfmt").arg(output_path).status()?;

    if !fmt_status.success() {
        eprintln!("warning: rustfmt was not successful: {fmt_status}");
    }

    Ok(())
}
