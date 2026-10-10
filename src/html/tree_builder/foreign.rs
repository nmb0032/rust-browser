use super::TreeBuilder;
use crate::dom::{Attribute, ElementData, Namespace};
use crate::html::{
    parser::HtmlError,
    tokenizer::{TagToken, Token},
};

impl TreeBuilder {
    fn adjusted_current(&self) -> Option<&ElementData> {
        if self.open.len() == 1 && self.fragment.is_some() {
            return self.fragment.as_ref();
        }
        self.current()
            .and_then(|id| self.document.node(id))
            .and_then(|node| node.as_element())
    }

    pub fn allow_cdata(&self) -> bool {
        self.adjusted_current()
            .is_some_and(|element| element.namespace() != &Namespace::Html)
    }

    pub(super) fn use_foreign_rules(&self, token: &Token) -> bool {
        let Some(element) = self.adjusted_current() else {
            return false;
        };
        if element.namespace() == &Namespace::Html || matches!(token, Token::Eof) {
            return false;
        }
        let character = matches!(token, Token::Character(_));
        let start = matches!(token, Token::StartTag(_));
        if math_integration(element)
            && (character
                || matches!(token, Token::StartTag(tag) if !matches!(tag.name.as_str(), "mglyph" | "malignmark")))
        {
            return false;
        }
        if element.namespace() == &Namespace::MathMl
            && element.tag_name() == "annotation-xml"
            && matches!(token, Token::StartTag(tag) if tag.name == "svg")
        {
            return false;
        }
        !(html_integration(element) && (character || start))
    }

    pub(super) fn foreign(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match token {
            Token::Character(text) => {
                if text == "\0" {
                    self.error("unexpected-null-in-foreign-content");
                    self.text("\u{FFFD}")?;
                } else {
                    self.text(text)?;
                    if !super::space(text) {
                        self.frameset_ok = false;
                    }
                }
            }
            Token::Comment(text) => self.comment(text, None)?,
            Token::Doctype(_) => self.error("unexpected-doctype"),
            Token::StartTag(tag) if breakout(tag) => {
                self.error("html-start-tag-in-foreign-content");
                self.pop_foreign();
                return self.html_rules(token);
            }
            Token::StartTag(tag) => {
                let namespace = self
                    .adjusted_current()
                    .ok_or(HtmlError::InvalidTreeState)?
                    .namespace()
                    .clone();
                self.insert(tag, namespace)?;
                if tag.self_closing {
                    self.pop();
                }
            }
            Token::EndTag(tag) if matches!(tag.name.as_str(), "br" | "p") => {
                self.error("html-end-tag-in-foreign-content");
                self.pop_foreign();
                return self.html_rules(token);
            }
            Token::EndTag(tag) => {
                if !self.current_name().eq_ignore_ascii_case(&tag.name) {
                    self.error("misnested-foreign-end-tag");
                }
                for index in (0..self.open.len()).rev() {
                    let id = self.open[index];
                    let element = self
                        .document
                        .node(id)
                        .and_then(|node| node.as_element())
                        .ok_or(HtmlError::InvalidTreeState)?;
                    if element.namespace() == &Namespace::Html {
                        return Ok(false);
                    }
                    if element.tag_name().eq_ignore_ascii_case(&tag.name) {
                        self.open.truncate(index);
                        return Ok(true);
                    }
                }
            }
            Token::Eof => return Ok(false),
            Token::ProcessingInstruction { .. } => return Err(HtmlError::InvalidTreeState),
        }
        Ok(true)
    }

    fn pop_foreign(&mut self) {
        while let Some(id) = self.current() {
            let Some(element) = self.document.node(id).and_then(|node| node.as_element()) else {
                break;
            };
            if element.namespace() == &Namespace::Html
                || math_integration(element)
                || html_integration(element)
            {
                break;
            }
            self.pop();
        }
    }

    pub(super) fn adjust_element(&self, tag: &TagToken, namespace: Namespace) -> ElementData {
        let name = if namespace == Namespace::Svg {
            SVG_NAMES
                .iter()
                .find(|(lower, _)| *lower == tag.name)
                .map_or(tag.name.as_str(), |(_, camel)| *camel)
                .to_owned()
        } else {
            tag.name.clone()
        };
        let attributes = tag
            .attributes
            .iter()
            .map(|attribute| {
                let mut name = attribute.name.clone();
                if namespace == Namespace::MathMl && name == "definitionurl" {
                    name = "definitionURL".into();
                }
                if namespace == Namespace::Svg
                    && let Some((_, camel)) =
                        SVG_ATTRIBUTES.iter().find(|(lower, _)| *lower == name)
                {
                    name = (*camel).into();
                }
                if namespace != Namespace::Html {
                    let (prefix, local, uri) = if let Some(local) =
                        name.strip_prefix("xlink:").filter(|local| {
                            matches!(
                                *local,
                                "actuate" | "arcrole" | "href" | "role" | "show" | "title" | "type"
                            )
                        }) {
                        (Some("xlink"), local, "http://www.w3.org/1999/xlink")
                    } else if let Some(local) = name
                        .strip_prefix("xml:")
                        .filter(|local| matches!(*local, "lang" | "space"))
                    {
                        (Some("xml"), local, "http://www.w3.org/XML/1998/namespace")
                    } else if name == "xmlns" {
                        (None, "xmlns", "http://www.w3.org/2000/xmlns/")
                    } else if name == "xmlns:xlink" {
                        (Some("xmlns"), "xlink", "http://www.w3.org/2000/xmlns/")
                    } else {
                        return Attribute::new(name, attribute.value.clone());
                    };
                    Attribute::namespaced(
                        local.into(),
                        attribute.value.clone(),
                        uri.into(),
                        prefix.map(str::to_owned),
                    )
                } else {
                    Attribute::new(name, attribute.value.clone())
                }
            })
            .collect();
        ElementData::namespaced(name, attributes, namespace, None)
    }
}

fn math_integration(element: &ElementData) -> bool {
    element.namespace() == &Namespace::MathMl
        && matches!(element.tag_name(), "mi" | "mo" | "mn" | "ms" | "mtext")
}
fn html_integration(element: &ElementData) -> bool {
    element.namespace() == &Namespace::Svg
        && matches!(element.tag_name(), "foreignObject" | "desc" | "title")
        || element.namespace() == &Namespace::MathMl
            && element.tag_name() == "annotation-xml"
            && element.attribute("encoding").is_some_and(|encoding| {
                encoding.eq_ignore_ascii_case("text/html")
                    || encoding.eq_ignore_ascii_case("application/xhtml+xml")
            })
}
fn breakout(tag: &TagToken) -> bool {
    matches!(
        tag.name.as_str(),
        "b" | "big"
            | "blockquote"
            | "body"
            | "br"
            | "center"
            | "code"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "em"
            | "embed"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "hr"
            | "i"
            | "img"
            | "li"
            | "listing"
            | "menu"
            | "meta"
            | "nobr"
            | "ol"
            | "p"
            | "pre"
            | "ruby"
            | "s"
            | "small"
            | "span"
            | "strong"
            | "strike"
            | "sub"
            | "sup"
            | "table"
            | "tt"
            | "u"
            | "ul"
            | "var"
    ) || tag.name == "font"
        && tag
            .attributes
            .iter()
            .any(|attribute| matches!(attribute.name.as_str(), "color" | "face" | "size"))
}

const SVG_NAMES: &[(&str, &str)] = &[
    ("altglyph", "altGlyph"),
    ("altglyphdef", "altGlyphDef"),
    ("altglyphitem", "altGlyphItem"),
    ("animatecolor", "animateColor"),
    ("animatemotion", "animateMotion"),
    ("animatetransform", "animateTransform"),
    ("clippath", "clipPath"),
    ("feblend", "feBlend"),
    ("fecolormatrix", "feColorMatrix"),
    ("fecomponenttransfer", "feComponentTransfer"),
    ("fecomposite", "feComposite"),
    ("feconvolvematrix", "feConvolveMatrix"),
    ("fediffuselighting", "feDiffuseLighting"),
    ("fedisplacementmap", "feDisplacementMap"),
    ("fedistantlight", "feDistantLight"),
    ("fedropshadow", "feDropShadow"),
    ("feflood", "feFlood"),
    ("fefunca", "feFuncA"),
    ("fefuncb", "feFuncB"),
    ("fefuncg", "feFuncG"),
    ("fefuncr", "feFuncR"),
    ("fegaussianblur", "feGaussianBlur"),
    ("feimage", "feImage"),
    ("femerge", "feMerge"),
    ("femergenode", "feMergeNode"),
    ("femorphology", "feMorphology"),
    ("feoffset", "feOffset"),
    ("fepointlight", "fePointLight"),
    ("fespecularlighting", "feSpecularLighting"),
    ("fespotlight", "feSpotLight"),
    ("fetile", "feTile"),
    ("feturbulence", "feTurbulence"),
    ("foreignobject", "foreignObject"),
    ("glyphref", "glyphRef"),
    ("lineargradient", "linearGradient"),
    ("radialgradient", "radialGradient"),
    ("textpath", "textPath"),
];
const SVG_ATTRIBUTES: &[(&str, &str)] = &[
    ("attributename", "attributeName"),
    ("attributetype", "attributeType"),
    ("basefrequency", "baseFrequency"),
    ("baseprofile", "baseProfile"),
    ("calcmode", "calcMode"),
    ("clippathunits", "clipPathUnits"),
    ("diffuseconstant", "diffuseConstant"),
    ("edgemode", "edgeMode"),
    ("filterunits", "filterUnits"),
    ("glyphref", "glyphRef"),
    ("gradienttransform", "gradientTransform"),
    ("gradientunits", "gradientUnits"),
    ("kernelmatrix", "kernelMatrix"),
    ("kernelunitlength", "kernelUnitLength"),
    ("keypoints", "keyPoints"),
    ("keysplines", "keySplines"),
    ("keytimes", "keyTimes"),
    ("lengthadjust", "lengthAdjust"),
    ("limitingconeangle", "limitingConeAngle"),
    ("markerheight", "markerHeight"),
    ("markerunits", "markerUnits"),
    ("markerwidth", "markerWidth"),
    ("maskcontentunits", "maskContentUnits"),
    ("maskunits", "maskUnits"),
    ("numoctaves", "numOctaves"),
    ("pathlength", "pathLength"),
    ("patterncontentunits", "patternContentUnits"),
    ("patterntransform", "patternTransform"),
    ("patternunits", "patternUnits"),
    ("pointsatx", "pointsAtX"),
    ("pointsaty", "pointsAtY"),
    ("pointsatz", "pointsAtZ"),
    ("preservealpha", "preserveAlpha"),
    ("preserveaspectratio", "preserveAspectRatio"),
    ("primitiveunits", "primitiveUnits"),
    ("refx", "refX"),
    ("refy", "refY"),
    ("repeatcount", "repeatCount"),
    ("repeatdur", "repeatDur"),
    ("requiredextensions", "requiredExtensions"),
    ("requiredfeatures", "requiredFeatures"),
    ("specularconstant", "specularConstant"),
    ("specularexponent", "specularExponent"),
    ("spreadmethod", "spreadMethod"),
    ("startoffset", "startOffset"),
    ("stddeviation", "stdDeviation"),
    ("stitchtiles", "stitchTiles"),
    ("surfacescale", "surfaceScale"),
    ("systemlanguage", "systemLanguage"),
    ("tablevalues", "tableValues"),
    ("targetx", "targetX"),
    ("targety", "targetY"),
    ("textlength", "textLength"),
    ("viewbox", "viewBox"),
    ("viewtarget", "viewTarget"),
    ("xchannelselector", "xChannelSelector"),
    ("ychannelselector", "yChannelSelector"),
    ("zoomandpan", "zoomAndPan"),
];
