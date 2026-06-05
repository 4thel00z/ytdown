# Changelog

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
