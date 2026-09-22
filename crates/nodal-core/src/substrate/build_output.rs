//! Where a build writes, read from what the project states and never from a guess.
//!
//! `cargo build` names its output directory by its profile, and [`super::warmth`] reads
//! that in place. Every other build writes wherever the tool it runs is told to, so the
//! directory is read from the three places a project states it, in this order: the
//! build command the recipe names, the `package.json` script that command runs, and
//! the outputs the task cache declares for `build`. A build that names none of them
//! has no directory this can check, and the caller says so rather than looking at a
//! path the build never wrote.
//!
//! A command line names a directory in three forms. An output flag names the word
//! after it, or the part after `=`. A tool with one fixed output names it: `next build`
//! writes `.next`. And a word that is a path names itself: one of the conventional
//! output names, written bare or with `./` or a trailing `/`. The bare word `build` is
//! a script name and a subcommand before it is a directory, so it counts only when it
//! is written as a path or given to a flag.

use std::path::Path;

use crate::model::recipe::{PackageManager, Recipe, TaskCache};
use crate::recipe::infer::Project;

/// Flags whose value is the output directory, across the tools a `build` script runs.
const OUTPUT_FLAGS: &[&str] =
    &["--outDir", "--out-dir", "--outdir", "--output", "--output-path", "--dist-dir", "-o"];

/// Tools that write one directory unless a flag says otherwise, and the directory.
const TOOL_DEFAULTS: &[(&str, &str)] = &[
    ("next", ".next"),
    ("nuxt", ".output"),
    ("vite", "dist"),
    ("webpack", "dist"),
    ("parcel", "dist"),
    ("tsup", "dist"),
    ("astro", "dist"),
    ("react-scripts", "build"),
];

/// Output names that name a directory when written bare.
const BARE_NAMES: &[&str] = &["dist", "out", ".next", ".output"];

/// Output names that name a directory only when written as a path.
const PATH_ONLY_NAMES: &[&str] = &["build"];

/// The directory the build of `recipe` writes under `tree`, when it names one.
///
/// `command` is the recipe's build command, already split into words.
#[must_use]
pub fn named(recipe: &Recipe, tree: &Path, command: &[&str]) -> Option<String> {
    if let Some(directory) = in_words(command) {
        return Some(directory);
    }
    if let Some(body) = script_body(tree, command) {
        let words: Vec<&str> = body.split_whitespace().collect();
        if let Some(directory) = in_words(&words) {
            return Some(directory);
        }
    }
    recipe.task_cache.and_then(|cache| cache_output(cache, tree))
}

/// The directory these words name, by flag, by the tool's default, or as a path.
fn in_words(words: &[&str]) -> Option<String> {
    let mut previous: Option<&str> = None;
    for word in words {
        if previous.is_some_and(|flag| OUTPUT_FLAGS.contains(&flag)) {
            return Some(as_directory(word));
        }
        if let Some((flag, value)) = word.split_once('=')
            && OUTPUT_FLAGS.contains(&flag)
        {
            return Some(as_directory(value));
        }
        if let Some(directory) = path_word(word) {
            return Some(directory);
        }
        previous = Some(word);
    }
    words.iter().find_map(|word| {
        TOOL_DEFAULTS.iter().find(|(tool, _)| tool == word).map(|(_, dir)| (*dir).to_owned())
    })
}

/// A word that is itself an output directory, or nothing.
fn path_word(word: &str) -> Option<String> {
    let written_as_path = word.starts_with("./") || word.ends_with('/');
    let name = as_directory(word);
    if BARE_NAMES.contains(&name.as_str())
        || (written_as_path && PATH_ONLY_NAMES.contains(&name.as_str()))
    {
        return Some(name);
    }
    None
}

/// A word as a directory relative to the tree: no `./`, no trailing `/`.
fn as_directory(word: &str) -> String {
    word.trim_start_matches("./").trim_end_matches('/').to_owned()
}

/// The body of the `package.json` script `command` runs, when it runs one.
///
/// `npm run build`, `pnpm build`, `yarn build` and `bun run build` all run the script
/// named by the first word after the manager that is not `run` and not a flag. A flag's
/// value before the script is not read: `pnpm --filter web build` names `web` here,
/// finds no such script, and reads no directory, which is the cautious answer.
fn script_body(tree: &Path, command: &[&str]) -> Option<String> {
    let (program, rest) = command.split_first()?;
    [PackageManager::Npm, PackageManager::Pnpm, PackageManager::Yarn, PackageManager::Bun]
        .into_iter()
        .find(|manager| manager.program() == *program)?;
    let script = rest.iter().find(|word| *word != &"run" && !word.starts_with('-'))?;
    Project::open(tree).scripts().remove(*script)
}

/// The first directory the task cache declares as an output of `build`.
///
/// Turborepo states outputs as globs under `tasks.build.outputs` (`pipeline` before
/// version 2); Nx under `targetDefaults.build.outputs`, with `{projectRoot}/` in front.
/// A glob names its first path segment; an exclusion (`!`) names nothing.
fn cache_output(cache: TaskCache, tree: &Path) -> Option<String> {
    let (file, keys): (&str, &[&[&str]]) = match cache {
        TaskCache::Turborepo => {
            ("turbo.json", &[&["tasks", "build", "outputs"], &["pipeline", "build", "outputs"]])
        }
        TaskCache::Nx => ("nx.json", &[&["targetDefaults", "build", "outputs"]]),
    };
    let json = Project::open(tree).read_json(file)?;
    let outputs = keys
        .iter()
        .find_map(|path| path.iter().try_fold(&json, |value, key| value.get(key))?.as_array())?;
    outputs.iter().filter_map(serde_json::Value::as_str).find_map(first_segment)
}

/// The first path segment of an output glob, or nothing for an exclusion.
fn first_segment(glob: &str) -> Option<String> {
    if glob.starts_with('!') {
        return None;
    }
    let path = glob.trim_start_matches("{projectRoot}/").trim_start_matches("./");
    let segment =
        path.split('/').next().filter(|segment| !segment.is_empty() && !segment.contains('*'))?;
    Some(segment.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::named;
    use crate::model::recipe::{Recipe, TaskCache};

    fn words(line: &str) -> Vec<&str> {
        line.split_whitespace().collect()
    }

    fn of(line: &str) -> Option<String> {
        named(&Recipe::default(), Path::new("/nonexistent"), &words(line))
    }

    fn with_cache(cache: TaskCache) -> Recipe {
        Recipe { task_cache: Some(cache), ..Recipe::default() }
    }

    #[test]
    fn an_output_flag_names_the_directory_after_it_or_after_the_equals() {
        assert_eq!(of("tsc --outDir out").as_deref(), Some("out"));
        assert_eq!(of("tsc --outDir=lib/").as_deref(), Some("lib"));
        assert_eq!(of("esbuild src/index.ts --outdir ./public").as_deref(), Some("public"));
    }

    #[test]
    fn a_tool_with_one_output_names_it_and_a_flag_wins_over_it() {
        assert_eq!(of("next build").as_deref(), Some(".next"));
        assert_eq!(of("vite build").as_deref(), Some("dist"));
        assert_eq!(of("react-scripts build").as_deref(), Some("build"));
        assert_eq!(of("vite build --outDir www").as_deref(), Some("www"));
    }

    /// `build` is the script every one of these commands runs, so as a bare word it
    /// names no directory; written as a path it does.
    #[test]
    fn the_bare_word_build_is_a_script_name_and_a_path_is_a_directory() {
        assert_eq!(of("npm run build"), None);
        assert_eq!(of("turbo run build"), None);
        assert_eq!(of("rm -rf build/ && tsc").as_deref(), Some("build"));
        assert_eq!(of("cp -r static ./dist").as_deref(), Some("dist"));
    }

    #[test]
    fn the_script_the_manager_runs_is_read_from_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("package.json"),
            r#"{ "scripts": { "build": "tsc && vite build", "compile": "tsc --outDir lib" } }"#,
        )
        .unwrap();
        let read = |line: &str| named(&Recipe::default(), root.path(), &words(line));
        assert_eq!(read("npm run build").as_deref(), Some("dist"));
        assert_eq!(read("pnpm build").as_deref(), Some("dist"));
        assert_eq!(read("yarn compile").as_deref(), Some("lib"));
        assert_eq!(read("bun run --silent build").as_deref(), Some("dist"));
        assert_eq!(read("npm run test"), None, "a script that is not there names nothing");
    }

    #[test]
    fn the_task_cache_outputs_answer_when_nothing_else_names_a_directory() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("turbo.json"),
            r#"{ "tasks": { "build": { "outputs": ["!.next/cache/**", ".next/**"] } } }"#,
        )
        .unwrap();
        let turbo =
            named(&with_cache(TaskCache::Turborepo), root.path(), &words("turbo run build"));
        assert_eq!(turbo.as_deref(), Some(".next"));

        std::fs::write(
            root.path().join("nx.json"),
            r#"{ "targetDefaults": { "build": { "outputs": ["{projectRoot}/dist"] } } }"#,
        )
        .unwrap();
        let nx = named(&with_cache(TaskCache::Nx), root.path(), &words("nx run-many -t build"));
        assert_eq!(nx.as_deref(), Some("dist"));

        let unread = named(&Recipe::default(), root.path(), &words("turbo run build"));
        assert_eq!(unread, None, "a cache the recipe does not name is not read");
    }
}
