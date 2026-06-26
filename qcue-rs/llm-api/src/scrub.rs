// QCue S1-R47 — strip forged tool-call wrappers emitted as plain text.
const FORGED: &[&str] = &[
    "[TOOL_CALL]",
    "<tool_call>",
    "</tool_call>",
    "<invoke ",
    "<function_calls>",
    "</function_calls>",
];

/// Remove forged tool-call wrapper substrings from streamed text (keeps surrounding prose).
pub fn scrub_forged_wrappers(text: &str) -> String {
    let mut out = text.to_string();
    // remove paired <tool_call>...</tool_call> bodies first, then any stray markers.
    while let (Some(a), Some(b)) = (out.find("<tool_call>"), out.find("</tool_call>")) {
        if b > a {
            out.replace_range(a..b + "</tool_call>".len(), "");
        } else {
            break;
        }
    }
    for m in FORGED {
        out = out.replace(m, "");
    }
    out
}
