# Google web translation

The app exposes Google translation as an explicit online service. It uses the unofficial `translate.googleapis.com/translate_a/single` endpoint and requires the user's online authorization. The route is fast in local smoke tests, but it is not an open-source API and may be rate limited or changed upstream.

The app does not store browser cookies, rotate accounts, or bypass provider limits. Existing `google` provider settings remain selected as Google; retired account-based providers migrate to local translation. The current provider decision is documented in `2026-10-20-provider-options.md`.
