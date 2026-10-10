use std::{error::Error, fmt::Write, fs, path::Path};

use rust_browser::{
    dom::{Document, NodeKind},
    html::parser::HtmlParser,
};

use super::{Counts, chunks, files};

#[derive(Debug)]
struct Case {
    input: String,
    expected: String,
    errors: usize,
    context: Option<String>,
    scripting: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    output: String,
    errors: Vec<rust_browser::html::tokenizer::ParseError>,
    dropped_errors: usize,
}

pub(super) fn run(path: &Path) -> Result<(Counts, Counts), Box<dyn Error>> {
    let mut documents = Counts::default();
    let mut fragments = Counts::default();
    for file in files(path, "dat")? {
        for (index, case) in parse_cases(&fs::read_to_string(&file)?)?.iter().enumerate() {
            let flags: &[bool] = match case.scripting {
                Some(true) => &[true],
                Some(false) => &[false],
                None => &[false, true],
            };
            for scripting in flags {
                let counts = if case.context.is_some() {
                    &mut fragments
                } else {
                    &mut documents
                };
                counts.total += 1;
                let whole = parse(&case.input, usize::MAX)?;
                for size in [1, 7, 64] {
                    assert_eq!(
                        parse(&case.input, size)?,
                        whole,
                        "{}: case {} (scripting {scripting}, context {:?}, chunk {size})",
                        file.display(),
                        index + 1,
                        case.context
                    );
                }
                // There is no contextual-fragment API yet; never report accidental
                // document-backend matches as fragment conformance.
                if case.context.is_some() {
                    counts.unsupported += 1;
                    continue;
                }
                let shape_matches = whole.output == case.expected;
                counts.shape_passed += usize::from(shape_matches);
                if shape_matches && whole.errors.len() + whole.dropped_errors == case.errors {
                    counts.passed += 1;
                }
            }
        }
    }
    if documents.total == 0 || fragments.total == 0 {
        return Err("tree fixtures must include both documents and fragments".into());
    }
    Ok((documents, fragments))
}

fn parse(source: &str, size: usize) -> Result<Outcome, Box<dyn Error>> {
    let mut parser = HtmlParser::new();
    for chunk in chunks(source, size) {
        parser.feed(chunk).map_err(|error| format!("{error:?}"))?;
    }
    let parsed = parser
        .finish_with_diagnostics()
        .map_err(|error| format!("{error:?}"))?;
    Ok(Outcome {
        output: serialize(&parsed.document),
        errors: parsed.errors,
        dropped_errors: parsed.dropped_errors,
    })
}

fn serialize(document: &Document) -> String {
    let mut output = String::new();
    // An explicit stack also lets deeply nested hostile fixtures run without
    // putting one recursive call per DOM node on the native stack.
    let mut pending = vec![(document.root_id(), 0)];
    while let Some((id, depth)) = pending.pop() {
        let node = document.node(id).expect("valid arena link");
        let indent = "  ".repeat(depth);
        match node.kind() {
            NodeKind::Document => {
                pending.extend(node.children().iter().rev().map(|id| (*id, depth)));
                continue;
            }
            NodeKind::Element(element) => {
                writeln!(output, "| {indent}<{}>", element.tag_name()).unwrap();
                let mut attributes = element.attributes().iter().collect::<Vec<_>>();
                attributes.sort_by(|left, right| {
                    left.name().encode_utf16().cmp(right.name().encode_utf16())
                });
                for attribute in attributes {
                    writeln!(
                        output,
                        "| {indent}  {}=\"{}\"",
                        attribute.name(),
                        attribute.value()
                    )
                    .unwrap();
                }
            }
            NodeKind::Text(text) => writeln!(output, "| {indent}\"{text}\"").unwrap(),
        }
        pending.extend(node.children().iter().rev().map(|id| (*id, depth + 1)));
    }
    output.strip_suffix('\n').unwrap_or(&output).to_owned()
}

fn parse_cases(source: &str) -> Result<Vec<Case>, Box<dyn Error>> {
    let mut cases = Vec::new();
    let mut lines = source.split_terminator('\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        if line != "#data" {
            return Err(format!("expected #data, got {line:?}").into());
        }
        let mut input = Vec::new();
        loop {
            let line = lines.next().ok_or("missing #errors")?;
            if line == "#errors" {
                break;
            }
            input.push(line);
        }
        let mut case = Case {
            input: input.join("\n"),
            expected: String::new(),
            errors: 0,
            context: None,
            scripting: None,
        };
        loop {
            match lines.next().ok_or("missing #document")? {
                "#document" => break,
                "#new-errors" => {}
                "#document-fragment" => {
                    case.context = Some(lines.next().ok_or("missing fragment context")?.to_owned());
                }
                "#script-on" => case.scripting = Some(true),
                "#script-off" => case.scripting = Some(false),
                line if !line.is_empty() => case.errors += 1,
                _ => return Err("unexpected empty metadata line".into()),
            }
        }
        let mut expected = Vec::new();
        while let Some(line) = lines.peek() {
            if *line == "#data" {
                break;
            }
            expected.push(lines.next().expect("peeked line"));
        }
        if expected.last() == Some(&"") {
            expected.pop();
        }
        case.expected = expected.join("\n");
        cases.push(case);
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_multiline_inputs_fragments_scripting_and_both_error_sections() {
        let cases = parse_cases(
            "#data\n<p>\nx\n#errors\none\n#new-errors\ntwo\n#document-fragment\ntable\n#script-off\n#document\n| \"x\"\n\n",
        ).unwrap();
        assert_eq!(cases[0].input, "<p>\nx");
        assert_eq!(cases[0].errors, 2);
        assert_eq!(cases[0].context.as_deref(), Some("table"));
        assert_eq!(cases[0].scripting, Some(false));
        assert_eq!(cases[0].expected, "| \"x\"");
    }

    #[test]
    fn empty_document_output_and_empty_input_are_preserved() {
        let cases =
            parse_cases("#data\n\n#errors\n#document\n\n#data\nx\n#errors\n#document\n| \"x\"\n")
                .unwrap();
        assert_eq!(cases.len(), 2);
        assert!(cases[0].input.is_empty());
        assert!(cases[0].expected.is_empty());
    }

    #[test]
    fn serialization_uses_literal_text_and_sorted_attributes() {
        let document = rust_browser::html::parser::parse("<p z=last a='first'>a\nb</p>").unwrap();
        assert_eq!(
            serialize(&document),
            "| <p>\n|   a=\"first\"\n|   z=\"last\"\n|   \"a\nb\""
        );
    }

    #[test]
    fn rejects_broken_fixture_sections() {
        assert!(parse_cases("#data\nx\n").is_err());
        assert!(parse_cases("#data\nx\n#errors\n").is_err());
    }
}
