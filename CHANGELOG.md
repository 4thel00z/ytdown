# Changelog

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
