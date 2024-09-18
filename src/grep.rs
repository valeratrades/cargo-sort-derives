use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::{overrides::OverrideBuilder, types::TypesBuilder, WalkBuilder};

const PATTERN: &str = r"#\[derive\([^\)]+\)\]";

pub type Matches = HashMap<PathBuf, HashSet<usize>>;

struct Match {
    file_path: PathBuf,
    line_number: usize,
}

pub fn grep<P: AsRef<Path>>(root: P, exclude: Vec<String>) -> Result<Matches, String> {
    let (tx, rx) = mpsc::channel();

    let mut type_builder = TypesBuilder::new();
    type_builder.add_defaults().select("rust");

    let mut override_builder = OverrideBuilder::new(root.as_ref());
    for glob in exclude {
        override_builder
            .add(&format!("!{}", glob))
            .map_err(|e| e.to_string())?;
    }

    let walker = WalkBuilder::new(root.as_ref())
        .standard_filters(true)
        .types(type_builder.build().unwrap())
        .overrides(override_builder.build().unwrap())
        .build_parallel();

    walker.run(|| {
        let tx = tx.clone();
        Box::new(move |result| match result {
            Ok(entry) => {
                if let Some(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        grep_file(entry.into_path(), &tx);
                    }
                }
                ignore::WalkState::Continue
            }
            Err(err) => {
                tx.send(Err(err.to_string())).unwrap();
                ignore::WalkState::Quit
            }
        })
    });

    drop(tx);

    let matches: Result<Vec<Match>, String> = rx.into_iter().collect();

    matches.map(|ms| {
        ms.into_iter()
            .fold(HashMap::<PathBuf, HashSet<usize>>::new(), |mut acc, m| {
                acc.entry(m.file_path).or_default().insert(m.line_number);
                acc
            })
    })
}

struct SearchSink<'a> {
    tx: &'a Sender<Result<Match, String>>,
    file_path: &'a Path,
}

impl<'a> Sink for SearchSink<'a> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let m = Match {
            file_path: self.file_path.to_owned(),
            line_number: mat.line_number().unwrap() as usize,
        };
        self.tx.send(Ok(m)).unwrap();
        Ok(true)
    }
}

fn grep_file(path: PathBuf, tx: &Sender<Result<Match, String>>) {
    let matcher = RegexMatcherBuilder::new().build(PATTERN).unwrap();

    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let sink = SearchSink {
        tx,
        file_path: &path,
    };
    if let Err(err) = searcher.search_path(&matcher, &path, sink) {
        tx.send(Err(err.to_string())).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use assert_fs::prelude::{FileWriteStr, PathChild};

    use super::*;

    #[test]
    fn test_grep() {
        let files = &[
            ("a.rs", rs_file_1(), true),
            ("b.rs", rs_file_1(), true),
            ("c.rs", rs_file_2(), false), // no derive
            ("x/xa.rs", rs_file_1(), true),
            ("x/xb.txt", rs_file_1(), false), // not .rs
            ("x/y/ya.rs", rs_file_1(), true),
            ("x/z/za.rs", rs_file_1(), true),
        ];
        let exclude = vec![];

        let tmp_root_dir = setup_tmp_files(files);
        let expected = expected_matches(tmp_root_dir.path(), files);

        let actual = grep(tmp_root_dir.path(), exclude).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_grep_with_exclude() {
        let files = &[
            ("a.rs", rs_file_1(), true),
            ("b.rs", rs_file_1(), false),
            ("x/xa.rs", rs_file_1(), false),
            ("x/xb.rs", rs_file_1(), false),
            ("x/y/ya.rs", rs_file_1(), false),
            ("x/z/za.rs", rs_file_1(), false),
            ("o/oa.rs", rs_file_1(), true),
            ("o/p/pa.rs", rs_file_1(), false),
            ("o/p/pb.rs", rs_file_1(), true),
            ("k/l/m/n/na.rs", rs_file_1(), false),
        ];
        let exclude = vec![
            "b.rs".into(),
            "x/*".into(),
            "pa.rs".into(),
            "k/**/na.rs".into(),
        ];

        let tmp_root_dir = setup_tmp_files(files);
        let expected = expected_matches(tmp_root_dir.path(), files);

        let actual = grep(tmp_root_dir.path(), exclude).unwrap();

        assert_eq!(actual, expected);
    }

    fn rs_file_1() -> (&'static str, HashSet<usize>) {
        let source = r#"
        #[derive(Debug)]
        struct A;

        #[derive(Clone, Copy)]
        struct B;
        "#;
        let derive_lines = HashSet::from([2, 5]);
        (source, derive_lines)
    }

    fn rs_file_2() -> (&'static str, HashSet<usize>) {
        let source = r#"
        struct A;
        "#;
        let derive_lines = HashSet::from([]);
        (source, derive_lines)
    }

    type Files<'a> = &'a [(&'a str, (&'a str, HashSet<usize>), bool)];

    fn setup_tmp_files(files: Files) -> assert_fs::TempDir {
        let tmp_root_dir = assert_fs::TempDir::new().unwrap();
        for (path, (content, _), _) in files.iter() {
            tmp_root_dir.child(path).write_str(content).unwrap();
        }
        tmp_root_dir
    }

    fn expected_matches(tmp_root_path: &Path, files: Files) -> Matches {
        files
            .iter()
            .filter(|(_, (_, _), is_match)| *is_match)
            .map(|(p, (_, ls), _)| (tmp_root_path.join(p), ls.iter().copied().collect()))
            .collect()
    }
}
