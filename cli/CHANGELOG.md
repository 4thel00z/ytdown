# Changelog

## [0.4.0](https://github.com/4thel00z/ytdown/compare/ytdown-cli-v0.3.1...ytdown-cli-v0.4.0) (2026-07-05)


### Features

* authenticate requests with browser cookies (--cookies) ([0a27250](https://github.com/4thel00z/ytdown/commit/0a27250fa0922dd8470010ba0f2535b611a64ec7))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ytdown bumped from 0.6.0 to 0.7.0

## [0.3.1](https://github.com/4thel00z/ytdown/compare/ytdown-cli-v0.3.0...ytdown-cli-v0.3.1) (2026-06-06)


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ytdown bumped from 0.5.0 to 0.6.0

## [0.3.0](https://github.com/4thel00z/ytdown/compare/ytdown-cli-v0.2.0...ytdown-cli-v0.3.0) (2026-06-06)


### Features

* **cli:** colorful animated download progress bars ([a09fa1c](https://github.com/4thel00z/ytdown/commit/a09fa1cb470c64646f774c9d5a9c02ac4c621669))
* **cli:** feature-gated 'serve' subcommand serving the embedded demo ([a831fbe](https://github.com/4thel00z/ytdown/commit/a831fbe782b3a99de4ba697da7db59b3bee13d7a))
* **cli:** serve /proxy streams upstream and restores Origin/Referer aliases ([e55d5b8](https://github.com/4thel00z/ytdown/commit/e55d5b87bc7580db803a4210eda3b9041d201865))


### Bug Fixes

* **cli:** serve build.rs degrades gracefully without wasm (unbreaks --all-features CI) ([8d61974](https://github.com/4thel00z/ytdown/commit/8d61974bedeaed3a9d417a8a24263ec5961e7bce))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ytdown bumped from 0.4.0 to 0.5.0

## [0.2.0](https://github.com/4thel00z/ytdown/compare/ytdown-cli-v0.1.0...ytdown-cli-v0.2.0) (2026-06-05)


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


### Bug Fixes

* **cli:** default selection must not self-merge progressive-only sources ([1acd41a](https://github.com/4thel00z/ytdown/commit/1acd41a01dc9b2c42d8fa43c3c6ab5e9fb94928c))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ytdown bumped from 0.3 to 0.4.0
