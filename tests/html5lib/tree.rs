use std::{error::Error, fmt::Write, fs, path::Path};

use rust_browser::{
    dom::{Attribute, Document, DocumentMode, ElementData, Namespace, NodeKind},
    html::parser::{FragmentContext, HtmlParser, ParserOptions},
};

use super::{Counts, chunks, files};

#[derive(Debug)]
struct Case {
    input: String,
    expected: String,
    errors: usize,
    current_errors: usize,
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
    let living = parse_cases(include_str!("living-standard.dat"))?;
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
                let whole = parse(&case.input, usize::MAX, case.context.as_deref(), *scripting)?;
                for size in [1, 7, 64] {
                    assert_eq!(
                        parse(&case.input, size, case.context.as_deref(), *scripting)?,
                        whole,
                        "{}: case {} (scripting {scripting}, context {:?}, chunk {size})",
                        file.display(),
                        index + 1,
                        case.context
                    );
                }
                let shape_matches = whole.output == case.expected;
                let current = living
                    .iter()
                    .find(|updated| updated.input == case.input && updated.context == case.context);
                assert_eq!(
                    whole.output,
                    current.unwrap_or(case).expected,
                    "current-spec tree output regression: {}:{} (context {:?}, scripting {scripting})",
                    file.display(),
                    index + 1,
                    case.context
                );
                if let Some(current) = current {
                    assert_eq!(
                        whole.errors.len() + whole.dropped_errors,
                        current.errors,
                        "living-standard PI diagnostic count"
                    );
                }
                counts.shape_passed += usize::from(shape_matches);
                let current_errors = current.map_or_else(
                    || {
                        case.current_errors
                            .checked_add_signed(super::expectations::tree_delta(
                                file.file_name()
                                    .and_then(|name| name.to_str())
                                    .expect("fixture filename"),
                                index + 1,
                            ))
                            .expect("audited diagnostic count")
                    },
                    |case| case.errors,
                );
                let current_matches = whole.output == current.unwrap_or(case).expected
                    && whole.errors.len() + whole.dropped_errors == current_errors;
                counts.current_passed += usize::from(current_matches);
                if !current_matches {
                    eprintln!(
                        "CURRENT {}:{} (scripting {scripting}, context {:?}) input {:?}\nexpected {current_errors} errors, actual {}:\n{:?}",
                        file.display(),
                        index + 1,
                        case.context,
                        case.input,
                        whole.errors.len() + whole.dropped_errors,
                        whole.errors
                    );
                }
                if shape_matches && whole.errors.len() + whole.dropped_errors == case.errors {
                    counts.passed += 1;
                } else if std::env::var_os("HTML5LIB_TREE_FAILURES").is_some() {
                    eprintln!(
                        "{}:{} (scripting {scripting}, context {:?}) input {:?}\nexpected ({} errors):\n{}\nactual ({} errors):\n{}\n{:?}\n",
                        file.display(),
                        index + 1,
                        case.context,
                        case.input,
                        case.errors,
                        case.expected,
                        whole.errors.len() + whole.dropped_errors,
                        whole.output,
                        whole.errors
                    );
                }
            }
        }
    }
    if documents.total == 0 || fragments.total == 0 {
        return Err("tree fixtures must include both documents and fragments".into());
    }
    Ok((documents, fragments))
}

fn parse(
    source: &str,
    size: usize,
    context: Option<&str>,
    scripting: bool,
) -> Result<Outcome, Box<dyn Error>> {
    let mut parser = if let Some(context) = context {
        let (namespace, name) = match context.split_once(' ') {
            Some(("svg", name)) => (Namespace::Svg, name),
            Some(("math", name)) => (Namespace::MathMl, name),
            Some(_) => return Err("unknown fragment namespace".into()),
            None => (Namespace::Html, context),
        };
        HtmlParser::fragment(FragmentContext {
            element: ElementData::namespaced(name.into(), Vec::new(), namespace, None),
            ancestors: Vec::new(),
            document_mode: DocumentMode::NoQuirks,
            scripting,
        })
        .map_err(|error| format!("{error:?}"))?
    } else {
        HtmlParser::with_options(ParserOptions { scripting })
    };
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
            NodeKind::Document | NodeKind::DocumentFragment => {
                pending.extend(node.children().iter().rev().map(|id| (*id, depth)));
                continue;
            }
            NodeKind::Element(element) => {
                let namespace = match element.namespace() {
                    Namespace::Svg => "svg ",
                    Namespace::MathMl => "math ",
                    _ => "",
                };
                writeln!(output, "| {indent}<{namespace}{}>", element.tag_name()).unwrap();
                let mut attributes = element.attributes().iter().collect::<Vec<_>>();
                attributes.sort_by(|left, right| {
                    attribute_name(left)
                        .encode_utf16()
                        .cmp(attribute_name(right).encode_utf16())
                });
                for attribute in attributes {
                    writeln!(
                        output,
                        "| {indent}  {}=\"{}\"",
                        attribute_name(attribute),
                        attribute.value()
                    )
                    .unwrap();
                }
                if let Some(contents) = element.template_contents() {
                    writeln!(output, "| {indent}  content").unwrap();
                    pending.push((contents, depth + 2));
                }
            }

            NodeKind::Text(text) => writeln!(output, "| {indent}\"{text}\"").unwrap(),
            NodeKind::Comment(text) => writeln!(output, "| {indent}<!-- {text} -->").unwrap(),
            NodeKind::Doctype(data) => {
                if data.public_identifier.is_empty() && data.system_identifier.is_empty() {
                    writeln!(output, "| {indent}<!DOCTYPE {}>", data.name).unwrap();
                } else {
                    writeln!(
                        output,
                        "| {indent}<!DOCTYPE {} \"{}\" \"{}\">",
                        data.name, data.public_identifier, data.system_identifier
                    )
                    .unwrap();
                }
            }
            NodeKind::ProcessingInstruction { target, data } => {
                writeln!(output, "| {indent}<?{target} {data}>").unwrap()
            }
        }
        pending.extend(node.children().iter().rev().map(|id| (*id, depth + 1)));
    }
    output.strip_suffix('\n').unwrap_or(&output).to_owned()
}

fn attribute_name(attribute: &Attribute) -> String {
    let prefix = match attribute.namespace() {
        Some("http://www.w3.org/1999/xlink") => "xlink ",
        Some("http://www.w3.org/XML/1998/namespace") => "xml ",
        Some("http://www.w3.org/2000/xmlns/") => "xmlns ",
        _ => "",
    };
    format!("{prefix}{}", attribute.name())
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
            current_errors: 0,
            context: None,
            scripting: None,
        };
        let mut legacy = Vec::new();
        let mut new = Vec::new();
        let mut new_errors = false;
        loop {
            match lines.next().ok_or("missing #document")? {
                "#document" => break,
                "#new-errors" => new_errors = true,
                "#document-fragment" => {
                    case.context = Some(lines.next().ok_or("missing fragment context")?.to_owned());
                }
                "#script-on" => case.scripting = Some(true),
                "#script-off" => case.scripting = Some(false),
                line if !line.is_empty() => {
                    case.errors += 1;
                    if new_errors {
                        new.push(line.to_owned());
                    } else {
                        legacy.push(line.to_owned());
                    }
                }
                _ => return Err("unexpected empty metadata line".into()),
            }
            case.current_errors = super::expectations::diagnostic_count(&legacy, &new);
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
            "| <html>\n|   <head>\n|   <body>\n|     <p>\n|       a=\"first\"\n|       z=\"last\"\n|       \"a\nb\""
        );
    }

    #[test]
    fn rejects_broken_fixture_sections() {
        assert!(parse_cases("#data\nx\n").is_err());
        assert!(parse_cases("#data\nx\n#errors\n").is_err());
    }
}
