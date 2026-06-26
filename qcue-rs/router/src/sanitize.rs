// QCue S1-R28, S1-R29 — untrusted fencing + reserved-tag escaping (ingest-XSS guard).
/// Wrap an untrusted blob so the model treats it as DATA, not instructions (tail-only, S1-R28).
pub fn fence_untrusted(origin: &str, body: &str) -> String {
    format!(
        "<untrusted_source origin=\"{}\">{}</untrusted_source>",
        origin,
        escape_reserved_tags(body)
    )
}

/// Neutralize reserved system tags literally present in ingested content (S1-R29).
pub fn escape_reserved_tags(input: &str) -> String {
    input
        .replace("<system-reminder>", "&lt;system-reminder&gt;")
        .replace("</system-reminder>", "&lt;/system-reminder&gt;")
        .replace("<untrusted_source>", "&lt;untrusted_source&gt;")
        .replace("</untrusted_source>", "&lt;/untrusted_source&gt;")
}
