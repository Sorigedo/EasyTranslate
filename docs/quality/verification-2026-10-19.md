# Verification — 2026-10-19

Environment: macOS 27.0.1 / Apple Silicon; Rust 1.97.1; Swift 6.4 Command Line Tools. The app requires macOS 26+. No full Xcode or paid translation account used.

## Verified

- 23 Rust tests cover draft invalidation, exact replacement guards, blank text, article splitting, bridge protocol, bounded cache, OCR operation invalidation, numeric warnings, shortcut parsing/migration and damaged config preservation.
- cargo fmt --check and cargo clippy --all-targets -- -D warnings pass.
- Real Carbon registration verifies conflict rollback, cleanup of partially registered keys and swapping bindings.
- Native AX fixture verifies reading, replacement and rejection of stale snapshots.
- Settings UI verifies duplicate rejection, saving custom Control+Shift+Q, persistence and restore-default behavior.
- Repeated GUI workflows verify default Option+A and custom keys, automatic translation, explicit button replacement, continuing edits, blank-draft disablement and Option+D selection translation without source mutation.
- Full screenshot shortcut/drag/OCR/translation passed using only a disposable fixture. A simulated drag initially ran before the capture UI was ready; increasing readiness wait and using intermediate drag events made the test pass.
- Two-line generated image OCR preserves line breaks and translates to Chinese. Initial system OCR model compilation exceeded 30 seconds; after initialization, repeated runs passed. No model/recognition behavior was patched to force success.
- Double-click packaging script builds the app and ZIP; icon contains 译; bundle plist and ad-hoc code signature verify.

## Not claimed

WeChat/WhatsApp version compatibility, input-method composition, multiple displays, Intel support, Apple notarization, and universal semantic correctness have not been validated. Free Apple local translation is fast in the short sample but mistranslates the payer in a payment instruction. This is a release limitation, not a passing accuracy result. No real chats were changed and no messages were sent by tests.


## Latest failing integration check

Repeated full screenshot drag check failed after earlier successes: captured OCR included incidental text (Ra / 截) before the English fixture. Per-paragraph automatic language detection then requested an unsupported/uninstalled language pair. This is unresolved; do not describe screenshot end-to-end behavior as stable. The optional --screenshot test preserves the failing reproduction; default/custom core workflow runs remain separate. Generated-image OCR passes.

Continuous-input GUI check also failed intermittently in two subsequent custom-key runs, then passed on the latest run with diagnostic capture enabled (no product fix). Cause is not established; this is not a stable repeated-pass claim. Settings duplicate validation and persistence passed throughout. Core integration requires further reliability work before production acceptance.
