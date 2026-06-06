# Changelog

## [0.5.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.4.0...ytdown-v0.5.0) (2026-06-06)


### Features

* **cli:** colorful animated download progress bars ([a09fa1c](https://github.com/4thel00z/ytdown/commit/a09fa1cb470c64646f774c9d5a9c02ac4c621669))
* **cli:** feature-gated 'serve' subcommand serving the embedded demo ([a831fbe](https://github.com/4thel00z/ytdown/commit/a831fbe782b3a99de4ba697da7db59b3bee13d7a))
* **cli:** serve /proxy streams upstream and restores Origin/Referer aliases ([e55d5b8](https://github.com/4thel00z/ytdown/commit/e55d5b87bc7580db803a4210eda3b9041d201865))
* **demo:** self-contained single-file HTML generator with preset + bulk UI ([8a814a4](https://github.com/4thel00z/ytdown/commit/8a814a487834c2aecd047f5d3d9bc7b7d258600d))
* **lib:** wasm-only build_with_transport for browser builds ([b774c19](https://github.com/4thel00z/ytdown/commit/b774c19c3019e94856238e1c4894f11173f63dc5))
* **proxy:** reference Cloudflare Worker passthrough with CORS ([15c12b1](https://github.com/4thel00z/ytdown/commit/15c12b15c65ec186e766965fd72932056593cb60))
* **transport:** add runtime-agnostic HttpClient trait and types ([11b773a](https://github.com/4thel00z/ytdown/commit/11b773a4c96ea863be733284b7244601130d9135))
* **transport:** native ReqwestClient with bounded streaming read ([edee7cf](https://github.com/4thel00z/ytdown/commit/edee7cf1d790298200cfe51aeac4ede6714b16ee))
* **wasm:** ytdown-wasm crate with JS-callback transport and resolve() ([9a87c2c](https://github.com/4thel00z/ytdown/commit/9a87c2c7a3c975ce73517d23fb7af32c44cff41b))
* **web:** downloadToDisk with FS Access streaming + Blob fallback ([5064dab](https://github.com/4thel00z/ytdown/commit/5064dab063f6216fa47f9d43b28a8179f712aca4))
* **web:** proxy fetch callback for the WASM transport ([dc32d7b](https://github.com/4thel00z/ytdown/commit/dc32d7bcc6994865b84e7606ffbf78b14a109941))
* **web:** Ytdown wrapper wiring proxy fetch into the wasm core ([c7748c4](https://github.com/4thel00z/ytdown/commit/c7748c4111daac8cbaa999d40a770921d317ede4))


### Bug Fixes

* **cli:** serve build.rs degrades gracefully without wasm (unbreaks --all-features CI) ([8d61974](https://github.com/4thel00z/ytdown/commit/8d61974bedeaed3a9d417a8a24263ec5961e7bce))
* **demo:** best-audio preset matches Rust (AudioOnly only, no progressive fallback) ([36b09e4](https://github.com/4thel00z/ytdown/commit/36b09e47e9a177b197576d9db963d2e7163e0807))
* **demo:** container ext, preset parity with FormatSelector, blob revoke + FS abort ([88fbfbf](https://github.com/4thel00z/ytdown/commit/88fbfbf1ac4eed2299851e3475b53d8cbd147129))
* **demo:** fsapi mechanism throws instead of silently falling back to Blob ([eed4fdb](https://github.com/4thel00z/ytdown/commit/eed4fdb8278c7e66e9986406b384a5a42a49c3dc))
* **download:** resume partials via bounded range chunks ([83c2064](https://github.com/4thel00z/ytdown/commit/83c2064d4ab2d9ed872e606271a4670f06c99619))
* **wasm:** defensive body decode + richer JS error context in transport ([79c9655](https://github.com/4thel00z/ytdown/commit/79c965557411341384d1834a6270ce74a992eb74))
* **web:** alias browser-forbidden Origin/Referer headers across SDK and proxy ([b6f846b](https://github.com/4thel00z/ytdown/commit/b6f846b4bcb360aa84661d6ca42ce9b723a9f924))

## [0.4.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.3.0...ytdown-v0.4.0) (2026-06-05)


### Features

* **cli:** -f format selector parsing and resolution ([701a4d3](https://github.com/4thel00z/ytdown/commit/701a4d3b2161a13958135d3734dc3b29dc388ae5))
* **cli:** async main with exit-code mapping and shared app plumbing ([1f2128b](https://github.com/4thel00z/ytdown/commit/1f2128b3650c6338e8186ef5925b4f33da924d17))
* **cli:** formats subcommand with table and JSON output ([0a1e381](https://github.com/4thel00z/ytdown/commit/0a1e3819b07f04811dd2f79692c8d9c0695b8eb7))
* **cli:** formats/search table rendering and humanizers ([5e30a75](https://github.com/4thel00z/ytdown/commit/5e30a759285c5548d4b55b9810cdfb7b013908ee))
* **cli:** get subcommand for single videos with merged downloads ([6b7aae8](https://github.com/4thel00z/ytdown/commit/6b7aae85ab3fbfb57a36a80d4651ed178d787be5))
* **cli:** indicatif progress wiring and tracing-over-bars writer ([7cb1c3d](https://github.com/4thel00z/ytdown/commit/7cb1c3def1becc9476b04a450f33a0dfe2a14fa8))
* **cli:** info subcommand with JSON output and wiremock e2e ([357b98e](https://github.com/4thel00z/ytdown/commit/357b98efbc98aa4e2aeeae270ccb77a217ae47d1))
* **cli:** interactive ratatui format picker for get ([7e2a142](https://github.com/4thel00z/ytdown/commit/7e2a1423e55bb768eba5220f266baeb86d20001f))
* **cli:** output filename templating with sanitization ([3b6a511](https://github.com/4thel00z/ytdown/commit/3b6a5119b7dfea3023768323e6e39f4c30d4105a))
* **cli:** playlist/channel/search downloads with skip/limit ([fff6986](https://github.com/4thel00z/ytdown/commit/fff69863acb65939a69fbcd358b4bbb4da33b3f0))
* **cli:** pure format-picker state machine ([6b578aa](https://github.com/4thel00z/ytdown/commit/6b578aadea4b3b5b6298c6a82d01013239b882df))
* **cli:** search subcommand ([4d6ea1b](https://github.com/4thel00z/ytdown/commit/4d6ea1b948b98ff9efb6e36af9ea0c72d06ac404))
* YtdownBuilder::clear_extractors for full extractor control ([fedd00b](https://github.com/4thel00z/ytdown/commit/fedd00b5b77445b2cc32d1b61c3bf2310d49d989))


### Bug Fixes

* **cli:** default selection must not self-merge progressive-only sources ([1acd41a](https://github.com/4thel00z/ytdown/commit/1acd41a01dc9b2c42d8fa43c3c6ab5e9fb94928c))
* **youtube:** defeat 403 PO-token gate and download throttling ([75e3eea](https://github.com/4thel00z/ytdown/commit/75e3eea3f73f7127f9de898eea2c81d7b1d7e66a))

## [0.3.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.2.0...ytdown-v0.3.0) (2026-06-05)


### Features

* add Python bindings (ytdown-py) and PyPI publish via CI ([de2f823](https://github.com/4thel00z/ytdown/commit/de2f823016408b0ce33bc62a1cde390ea1f5236c))

## [0.2.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.1.0...ytdown-v0.2.0) (2026-06-05)


### Features

* boa_engine-backed JS cipher function runner ([ba38877](https://github.com/4thel00z/ytdown/commit/ba38877aa6c72cbad6518c8148c4db85a6914644))
* chunked resumable downloader with progress reporting ([db81c9c](https://github.com/4thel00z/ytdown/commit/db81c9c7de0d587186e8de8dbb684668eaa65270))
* core error and media type contracts ([acfb8c6](https://github.com/4thel00z/ytdown/commit/acfb8c68f9aebd8179cecc317c6eb0e78d0db243))
* extractor trait and registry dispatch ([ca69ee5](https://github.com/4thel00z/ytdown/commit/ca69ee5440a18d6cb22285bb4180133c87911cab))
* fluent format selection ([9da59ae](https://github.com/4thel00z/ytdown/commit/9da59ae595858aa809053c592e778301f2c65174))
* innertube api client with client impersonation ([e8366a3](https://github.com/4thel00z/ytdown/commit/e8366a36bef048d5b818b832daa8c7b53d6737f8))
* lazy continuation-based pagination for collections ([311b9fc](https://github.com/4thel00z/ytdown/commit/311b9fc57587785cf759d438fd1ceb95afb86886))
* non_exhaustive data structs, builder chunk_size/retries, collection docs ([0b3e915](https://github.com/4thel00z/ytdown/commit/0b3e9151c0b03af463ed35f9ba9c035c6237fc5f))
* optional ffmpeg postprocessing for dash a/v muxing ([f5b4080](https://github.com/4thel00z/ytdown/commit/f5b408084c98358b73ceadb2e5877d64d826bb74))
* public Ytdown client api ([acdd5d9](https://github.com/4thel00z/ytdown/commit/acdd5d947b99ca4a9b61580b9c81d8fa0ee321e7))
* youtube extractor with cipher solving and collections ([2ac1fa7](https://github.com/4thel00z/ytdown/commit/2ac1fa7bc909ee44e06bb553112fefbdd9778313))
* youtube player sig/nsig extraction and solving ([9cb336c](https://github.com/4thel00z/ytdown/commit/9cb336c5274cc1d2dea757059165d3b63eb1c949))


### Bug Fixes

* bound pagination on no-progress, reject oversized partials, warn on nsig passthrough ([30084f3](https://github.com/4thel00z/ytdown/commit/30084f36837ea612c5941522b3f2eb6dbf26f25e))
* harden downloads, cipher solving, and InnerTube fidelity ([3be47db](https://github.com/4thel00z/ytdown/commit/3be47dbe28d00bc0102af5da48df6644776c8e03))
* refresh InnerTube player client versions to fix live video resolution ([0f1e101](https://github.com/4thel00z/ytdown/commit/0f1e1017b9dfa81c5279dc459ff190af1f74917c))
