# Translation service decision — 2026-10-21

## Available services

| Service | Data flow | Status |
| --- | --- | --- |
| Firefox Translations Marian via `fxtranslate` | Runs on this Mac. Mozilla model files are cached under `~/Library/Application Support/EasyTranslate/Models`; after download, translation text stays local | Default offline provider. The app preloads the configured target direction in a background thread at startup. The current Chinese-English routes use about 102 MiB of cached model files together. |
| Ollama | Sends text to the selected model through the local Ollama native API | Each installed model is a separate selectable service after an Ollama refresh; the app uses the local Ollama host and does not require online authorization. |
| Google | Sends text to Google's web translation endpoint | Explicitly selectable and requires online authorization; no API key is stored. |
| SiliconFlow | Sends text to the user-configured OpenAI-compatible chat-completions URL with the configured model and API Key | Legacy compatible service retained for existing configurations; starts with online consent disabled. |
| OpenAI / local Codex configuration | Reads `~/.codex/auth.json` and `~/.codex/config.toml`, then sends translation requests to the endpoint and model configured there | Explicitly selectable. An optional model override accepts the model name selected via Codex `/model`; blank follows the configured model. Missing or failed Codex access is shown in the translation result, without silently switching services. |

SiliconFlow has a lazy OpenAI / Codex fallback after an API error, timeout or missing SiliconFlow key, only when online translation consent is enabled. Directly choosing OpenAI / Codex does not invoke SiliconFlow as a fallback. All provider failures appear in the translation area; failures are not cached as translation output.

Legacy `deep_seek` and `deep_seek_pro` settings migrate to SiliconFlow. Legacy `llm` selection remains OpenAI / Codex. Existing Google settings remain Google; retired MyMemory, Apple, and account-based commercial provider settings migrate to local translation.

## Offline model assessment

The current app uses the Firefox Translations Marian/OPUS-MT route through `fxtranslate`. On this Mac (Apple M3 Pro, 18 GB unified memory), the cached `zh-Hans→en` and `en→zh-Hans` model files total 102 MiB. The local benchmark on 2026-10-21 measured cached cold route preparation at 2.9 and 3.3 seconds, with warm single-sentence samples at 36–74 ms. The benchmark exposed semantic errors such as translating “周五前” as “on Friday”. It is fast and compact, but should not be described as consistently high-quality.

NLLB-200 distilled 600M/1.3B and M2M100 are plausible local comparison candidates; larger multilingual families such as MADLAD-400 require a different inference runtime and model packaging. Their size or benchmark reputation alone cannot establish better Chinese-English product results. This machine has capacity to trial a larger model, but external model metadata was not reachable during this review and no comparable local corpus evaluation was run. Therefore Marian remains the shipped offline engine for now; a replacement requires quality review, cold-start and warm latency measurements, memory measurements, licensing checks, and offline packaging verification.

The startup preload runs asynchronously and loads the configured default Chinese-English direction without translating placeholder text. The normal window remains responsive while the model is loaded or downloaded. If that initial preparation fails, the error is reported and a later translation can retry through the same worker.

## Verification boundary

Deterministic local HTTP tests check the Ollama native `/api/tags` path, SiliconFlow saved endpoint/model/key path and the explicit Codex error path without contacting public services. No SiliconFlow key was available for a live quality or latency run. No larger local model was installed or benchmarked. Users should review deadlines, negation, amounts and names before inserting text.
