//! Per-provider translation of the three core ModelSettings knobs. The
//! output-token cap and timeout are read directly from `ModelSettings` by each
//! adapter; this module owns the shared thinking-effort translation.
//! @sergent-rs-providers/docs/providers.md

use sergent_rs_core::model::ThinkingEffort;

/// The native reasoning/thinking effort string for OpenAI, Anthropic, and
/// Ollama's OpenAI-compatible endpoint.
pub(crate) fn effort_str(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::Low => "low",
        ThinkingEffort::Medium => "medium",
        ThinkingEffort::High => "high",
    }
}
