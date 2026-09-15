use agentgraph::index::parser::LineIndex;

#[test]
fn line_index_matches_naive() {
    let src = "a\nbb\n\nccc\nd";
    let idx = LineIndex::new(src);
    let mut line = 1usize;
    for (i, ch) in src.char_indices() {
        assert_eq!(idx.line_of(i), line, "offset {i}");
        if ch == '\n' {
            line += 1;
        }
    }
    assert_eq!(idx.line_of(src.len()), line);
}

#[test]
fn line_index_empty() {
    let idx = LineIndex::new("");
    assert_eq!(idx.line_of(0), 1);
}
