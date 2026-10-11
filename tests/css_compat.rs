mod support;

use rust_browser::{
    css::{media::MediaEnvironment, values::Length},
    dom::DocumentMode,
    engine::{Frame, LoadOptions, load_page},
    style::BoxSizing,
};
use std::{path::Path, process::Command, sync::Arc};
use support::{Reply, Server};

fn id(frame: &Frame) -> rust_browser::dom::NodeId {
    frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.attribute("id") == Some("target"))
        })
        .unwrap()
}

#[test]
fn compatibility_external_import_conditions_cache_and_restyle_use_document_mode() {
    for (doctype, mode) in [
        ("", DocumentMode::Quirks),
        ("<!doctype html>", DocumentMode::NoQuirks),
        (
            "<!DOCTYPE HTML PUBLIC '-//W3C//DTD HTML 4.01 Transitional//EN' 'http://www.w3.org/TR/html4/loose.dtd'>",
            DocumentMode::LimitedQuirks,
        ),
    ] {
        let quirky = mode == DocumentMode::Quirks;
        let server = Server::new(vec![
            ("/", Reply::ok("text/html", format!("{doctype}<style>@import 'a.css' supports(width:12);</style><link rel='stylesheet' href='a.css'><link rel='stylesheet' href='a.css'><p id='target'>text</p>").into_bytes())),
            ("/a.css", Reply::redirect("/real.css")),
            ("/real.css", Reply::ok("text/css", b"@import 'b.css' supports(color:abc); @media (-webkit-min-device-pixel-ratio:2) {p{-webkit-text-fill-color:blue}}".to_vec())),
            ("/b.css", Reply::ok("text/css", b"p {--n:80;width:var(--n);height:30;padding:10;border:2px solid blue;color:123;-webkit-box-sizing:border-box}".to_vec())),
        ], if quirky {4} else {3});
        let mut frame =
            load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
        assert_eq!(frame.document.mode(), mode);
        let target = id(&frame);
        let p = frame.styles.get(target).unwrap();
        assert_eq!(p.width, quirky.then_some(Length::Px(80.0)));
        assert_eq!(p.color, Some(if quirky { 0x000123 } else { 0 }));
        assert_eq!(
            p.box_sizing,
            if quirky {
                BoxSizing::BorderBox
            } else {
                BoxSizing::ContentBox
            }
        );
        if quirky {
            assert!(
                frame.stylesheets.diagnostics.is_empty(),
                "{:?}",
                frame.stylesheets.diagnostics
            );
        }
        let retained = frame
            .stylesheets
            .stylesheets
            .iter()
            .map(|sheet| Arc::clone(&sheet.stylesheet))
            .collect::<Vec<_>>();
        let diagnostics = frame.stylesheets.diagnostics.clone();
        for resolution in [2.0, 1.0, 2.0] {
            frame.set_media_environment(MediaEnvironment {
                resolution_dppx: Some(resolution),
                ..MediaEnvironment::default()
            });
            let p = frame.styles.get(target).unwrap();
            assert_eq!(
                p.text_fill_color.to_srgb8().0,
                if resolution == 2.0 {
                    0x0000ff
                } else {
                    p.color.unwrap()
                }
            );
            assert_eq!(frame.stylesheets.diagnostics, diagnostics);
            for (ast, sheet) in retained.iter().zip(&frame.stylesheets.stylesheets) {
                assert!(Arc::ptr_eq(ast, &sheet.stylesheet));
            }
        }
        assert_eq!(
            server.finish(),
            if quirky {
                vec!["/", "/a.css", "/real.css", "/b.css"]
            } else {
                vec!["/", "/a.css", "/real.css"]
            }
        );
    }
}

#[test]
fn compatibility_cli_reports_used_geometry_and_paints_fill_not_color() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/compatibility.html");
    let dump = |format| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&path)
            .arg(format!("--dump={format}"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        assert!(stderr.contains("missing-doctype"), "{stderr}");
        String::from_utf8(output.stdout).unwrap()
    };
    let values = dump("values");
    assert_eq!(values, dump("values"));
    for expected in [
        "computed box-sizing: BorderBox",
        "used content-width: UsedLength(56.0)",
        "used content-height: UsedLength(6.0)",
        "computed -webkit-text-fill-color:",
    ] {
        assert!(values.contains(expected), "{expected}: {values}");
    }
    let style = dump("style");
    assert!(style.contains("color: Rgb(291)"), "{style}");
    assert!(
        style.contains("-webkit-text-fill-color: Rgb(255)"),
        "{style}"
    );
    let display = dump("display-list");
    for text in ["Before-", "Origin"] {
        assert!(display.contains(&format!("text: \"{text}\"")), "{display}");
    }
    assert!(display.contains("color: 255"), "{display}");
    assert!(display.contains("w: 80.0"), "{display}");
}
