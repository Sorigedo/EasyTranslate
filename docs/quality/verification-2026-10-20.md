# Verification — 2026-10-20

## Scope and desktop constraint

User explicitly requested background testing. No further synthetic keyboard/mouse or chat interactions are allowed while this constraint remains. Native GUI acceptance is outstanding. Earlier intermittent failures include verified foreground switches to WeChat and unexpected fixture edits; do not classify those as product success or silently weaken focus guards.

## Implemented fixes

- Retain the bound target on temporary AX unavailability only while the same application remains frontmost; invalidate translation and disable replacement until reread succeeds. A different control/application still unbinds.
- Detect source language from document context rather than each short OCR fragment. Strong whole-document evidence takes precedence; line voting handles repeated paragraphs. Regressions include OCR noise, repeated English paragraphs and Chinese lines with an English signature.
- Shared Rust translation worker used by UI and background integration tests; cancellation checked before cached results and between segments; progress includes completion.
- MyMemory official HTTPS API, explicit per-app enablement before online text transmission, provider-specific cache, 12/15-second network timeouts, no automatic provider fallback, quota/HTTP/empty response checks, safe plus-sign encoding.
- Online segmentation uses up to 500 UTF-8 bytes per paragraph chunk, preserving a whole paragraph when it fits and preferring sentence endings when splitting longer paragraphs. Original whitespace is preserved. No model availability check occurs for online translation.

## Evidence

27 deterministic Rust tests, formatting and clippy passed. Native Swift online protocol tests cover quota/status/empty failures, entity decoding, literal plus and 500-byte rejection. Background integration verifies actual online payment/multiline translations, service cache isolation and opt-in gating. Local integration verifies cancellation and 30 paragraphs with invoice numbers preserved. Build/ad-hoc signing verified. GUI service settings are compiled but not automatically exercised under the user's desktop constraint.

## Follow-up: optional language packs

The diagnostic CLI previously queried Apple model availability regardless of the selected provider. It now reports the configured provider and online consent state; online diagnostics do not invoke the bridge or send text. Offline diagnostics check the configured target language. A subprocess regression first failed against the old behavior, then passed for both online consent states. The full background suite passed with 28 Rust tests, formatting, clippy, native response tests and language detection. User settings were read for provider fields only and left unchanged; AGENTS.md remains unchanged.

## Follow-up: online paragraph context

The earlier conservative 125-scalar limit fragmented a 384-byte English paragraph into four requests. A regression reproduced four planned segments before the fix and one afterward. Online splitting now applies the actual 500-byte limit; boundary fixtures cover ASCII, Chinese, emoji, CRLF, blank lines and exact reassembly. Another failing regression proved a later word boundary was preferred over a complete sentence; online mode now prefers sentence boundaries. Apple splitting remains unchanged. A cached two-paragraph cancellation test verifies no partial result is returned and a new generation can retry successfully. The full background suite passed with 32 Rust tests, formatting, clippy and native checks. Live public fixtures verify the real provider worker, cache isolation, paragraph progress and invoice-number preservation; these checks are not a semantic-accuracy guarantee.

## Accuracy findings

### OCR follow-up and packaging

After the paragraph fix, the one-click packaging command completed with 32 passing Rust tests and successful ad-hoc signature verification. ZIP inspection found the executable, native helper, icon and Info.plist. The packaged language-detection regression passed. A generated-image OCR test timed out at 30 seconds on its first invocation; a subsequent invocation passed without changing code or extending the timeout. ANECompilerService logs show network compilation ending during this test period, consistent with initialization overhead, but this does not prove all OCR delays are explained. OCR cold-start latency remains unresolved; do not count the first failure as a pass. No desktop screenshots or synthetic input were used.

### Translation quality

Online benchmark: 12 public fixtures, zero request errors, median 1,201.5 ms, maximum 2,134 ms. Payment subject is preserved, but before end of work became before work. Conditional Friday wording is less precise. Neither request success nor the small corpus proves accuracy or a service-level guarantee.

## Source verification

Fetched on 2026-10-20 directly from official provider pages:
- https://mymemory.translated.net/doc/spec.php — official GET endpoint, maximum 500 UTF-8 bytes, machine-translation option.
- https://mymemory.translated.net/doc/usagelimits.php — anonymous 5,000 characters/day; do not bypass limits.
- https://mymemory.translated.net/terms-and-conditions — data processing, third-party MT, no availability/accuracy warranty; paragraph-oriented use.
- https://raw.githubusercontent.com/tisfeng/Easydict/dev/docs/user-docs/en/SERVICES.md — service categories; no-key does not imply offline or stable upstream.

No upstream source code copied; no paid account or shared credentials added. MyMemory is a hosted service, not an open-source engine. Free capacity and translation quality remain limitations.

## Screenshot cancellation follow-up

- CPU/default-device experiment against a generated English image: first CPU invocation 30.53s; subsequent default and CPU invocations about 0.13s. Both produced the expected two lines. Switching compute devices did not remove cold initialization, so production OCR settings remain unchanged. Cold-start latency is still unresolved.
- Existing cancellation invalidated OCR results but did not terminate the capture wait or the OCR subprocess. Added a shared operation cancellation token, cancellable child wait with kill/reap, and cancellation propagation to the native OCR bridge. Normal quit now waits for worker completion after cancelling, allowing temporary PNG removal before exit.
- Regression RED: waiting on a 30-second child ignored cancellation and returned success; after repair it returned cancellation and reaped the process in under the 3-second test budget. A separate RED→GREEN regression proves starting a new operation updates a token held by the previous worker.
- 35 Rust tests and clippy passed. No GUI automation or real screenshots were used. The new cancellation control and quit path are compiled but still require GUI acceptance. Generic bridge failure/timeout messages no longer incorrectly prescribe a language-pack download for OCR or online failures.
- Focused paragraph review found no major issue; sentence boundaries remain a punctuation heuristic, especially around abbreviations and quotations.

Focused cancellation review found one important quit race: ordinary menu actions could start new work after quitting while a worker was winding down. Fixed by rejecting user action dispatch through with_app once quitting; the event loop calls tick independently so completion notifications and cleanup continue. This gate was statically reviewed and compiled, but the exact menu-during-quit sequence has not been exercised under the background-only constraint. Screenshot deletion errors are currently ignored and remain a limitation; no absolute deletion guarantee is claimed.

## Network failure and quality follow-up

The existing MyMemory deadline mistranslation reproduced again via its official API; its machine-translation match confirms the incorrect deadline is upstream, not a UI or paragraph-splitting defect. Provider options and unverified registration prerequisites are recorded separately in 2026-10-20-provider-options.md. The user has been asked whether free API registration is acceptable; no credentials, accounts or billing changes were made.

Network errors now map to actionable Chinese messages without embedding request URLs/text. A new regression first failed on the raw NSURLErrorDomain message and passed after mapping. URLProtocol fixtures exercise the production URLSession transport with offline, 429, 503 and malformed responses followed by recovery; errors are rejected, one request occurs per attempt, plus-sign encoding is preserved, and there is no automatic retry or provider fallback. These tests consume no public quota and are part of scripts/test-background.sh. Full background suite passed, including 35 Rust tests and native transport checks. This verifies request/error behavior, not provider uptime or semantic accuracy.

## OCR coverage and AX string integrity

- Generated Chinese image containing payment negation, an end-of-work deadline, order AB-2026 and amount 1,500.25 was recognized correctly; initial probe took 15.864s. The expanded OCR suite tests Chinese, English, blank, invalid and missing images. It no longer requires Apple translation models by default; the optional --apple-translation flag retains that separate integration test.
- The first expanded run failed exact whitespace equality: Vision omitted one space between Chinese text and a number. This is recorded as OCR spacing variance, not a character-recognition failure. The Chinese assertion now ignores ASCII spaces only while comparing every character, punctuation mark and line order. English remains exact. No production OCR output is normalized. The next full run passed (English 0.176s, Chinese 0.208s, blank 0.079s, invalid/missing about 0.02s). This does not remove the known cold-start latency.
- AX text conversion used a zero-terminated C string and truncated drafts containing U+0000. A native CoreFoundation roundtrip regression reproduced truncation of the suffix after U+0000. Switched to length-aware CFStringGetBytes; the regression now preserves Chinese, emoji, newlines, embedded null and empty strings. No accessibility calls to user applications were made.
- Full background checks passed with 36 Rust tests, formatting, clippy, response/transport checks and language detection.
- Read only application Info.plist: installed WeChat 4.1.15 and WhatsApp 26.41.28. Their presence/version is not compatibility evidence. No application activation, chat reading, synthetic input or screen capture occurred.

## Packaging verification uses current source

Previously scripts/test-background.sh ran detection against an existing dist helper and did not compile the current bridge. The root packaging command ran only Rust tests before building. Detection now accepts an explicit --bridge path and rejects missing paths; the original script ignored that argument and incorrectly passed against an old bundle, reproduced before the fix. Background checks compile current Swift sources into .build/TranslationBridge and test that path. One-click packaging runs the background suite, builds/signs the app, tests its bundled helper, then creates ZIP.

The complete command passed with 36 Rust tests, native response/transport checks, current-source and bundled detection, release build and signature verification. ZIP entries exactly match the current executable, helper, icon and plist. A direct .build-vs-bundle helper byte comparison initially failed due to code-signature differences; stripping signatures from temporary copies proved executable content equality. No delivered file was modified by that comparison. No GUI was activated.

A temporary source copy containing no dist or .build completed scripts/test-background.sh successfully and created only its own .build helper. Cargo dependencies/artifacts were reused from the workspace target directory; this verifies independence from old native artifacts, not a fully offline first-time dependency install. The temporary copy was removed afterward.
