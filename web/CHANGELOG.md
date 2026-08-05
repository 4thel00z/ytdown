# Changelog

## [0.6.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.5.0...ytdown-v0.6.0) (2026-08-05)


### Features

* add Reddit, TikTok, Instagram, and X/Twitter extractors ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **cookies:** derive the X-CSRFToken header from a csrftoken cookie for Instagram ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **download:** formats carry required HTTP headers (Cookie/Referer/UA) that the downloader now sends with every request ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **instagram:** extract posts and reels via the GraphQL shortcode query (cookies-aware, share links supported) ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **reddit:** extract v.redd.it post videos as split DASH formats plus fallback ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **tiktok:** extract tiktok.com videos, including vm./vt./t share shortlinks ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **transport:** report the post-redirect final URL on HTTP responses ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **twitter:** extract tweet videos via the public syndication API (no login) ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))


### Bug Fixes

* **cli:** make the bot-check hint site-agnostic instead of YouTube-specific ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))
* **http:** default the HTTP client to platform TLS — Reddit's edge blocklists the rustls ClientHello fingerprint ([9a1292e](https://github.com/4thel00z/ytdown/commit/9a1292ee6e0f4ec2d524dff7c62e8a97f93f210f))

## [0.5.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.4.0...ytdown-v0.5.0) (2026-07-05)


### Features

* **demo:** self-contained single-file HTML generator with preset + bulk UI ([8a814a4](https://github.com/4thel00z/ytdown/commit/8a814a487834c2aecd047f5d3d9bc7b7d258600d))
* **proxy:** reference Cloudflare Worker passthrough with CORS ([15c12b1](https://github.com/4thel00z/ytdown/commit/15c12b15c65ec186e766965fd72932056593cb60))
* **web:** downloadToDisk with FS Access streaming + Blob fallback ([5064dab](https://github.com/4thel00z/ytdown/commit/5064dab063f6216fa47f9d43b28a8179f712aca4))
* **web:** proxy fetch callback for the WASM transport ([dc32d7b](https://github.com/4thel00z/ytdown/commit/dc32d7bcc6994865b84e7606ffbf78b14a109941))
* **web:** Ytdown wrapper wiring proxy fetch into the wasm core ([c7748c4](https://github.com/4thel00z/ytdown/commit/c7748c4111daac8cbaa999d40a770921d317ede4))


### Bug Fixes

* **demo:** best-audio preset matches Rust (AudioOnly only, no progressive fallback) ([36b09e4](https://github.com/4thel00z/ytdown/commit/36b09e47e9a177b197576d9db963d2e7163e0807))
* **demo:** container ext, preset parity with FormatSelector, blob revoke + FS abort ([88fbfbf](https://github.com/4thel00z/ytdown/commit/88fbfbf1ac4eed2299851e3475b53d8cbd147129))
* **demo:** fsapi mechanism throws instead of silently falling back to Blob ([eed4fdb](https://github.com/4thel00z/ytdown/commit/eed4fdb8278c7e66e9986406b384a5a42a49c3dc))
* **web:** alias browser-forbidden Origin/Referer headers across SDK and proxy ([b6f846b](https://github.com/4thel00z/ytdown/commit/b6f846b4bcb360aa84661d6ca42ce9b723a9f924))

## [0.4.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.3.0...ytdown-v0.4.0) (2026-06-22)


### Features

* **demo:** self-contained single-file HTML generator with preset + bulk UI ([8a814a4](https://github.com/4thel00z/ytdown/commit/8a814a487834c2aecd047f5d3d9bc7b7d258600d))
* **proxy:** reference Cloudflare Worker passthrough with CORS ([15c12b1](https://github.com/4thel00z/ytdown/commit/15c12b15c65ec186e766965fd72932056593cb60))
* **web:** downloadToDisk with FS Access streaming + Blob fallback ([5064dab](https://github.com/4thel00z/ytdown/commit/5064dab063f6216fa47f9d43b28a8179f712aca4))
* **web:** proxy fetch callback for the WASM transport ([dc32d7b](https://github.com/4thel00z/ytdown/commit/dc32d7bcc6994865b84e7606ffbf78b14a109941))
* **web:** Ytdown wrapper wiring proxy fetch into the wasm core ([c7748c4](https://github.com/4thel00z/ytdown/commit/c7748c4111daac8cbaa999d40a770921d317ede4))


### Bug Fixes

* **demo:** best-audio preset matches Rust (AudioOnly only, no progressive fallback) ([36b09e4](https://github.com/4thel00z/ytdown/commit/36b09e47e9a177b197576d9db963d2e7163e0807))
* **demo:** container ext, preset parity with FormatSelector, blob revoke + FS abort ([88fbfbf](https://github.com/4thel00z/ytdown/commit/88fbfbf1ac4eed2299851e3475b53d8cbd147129))
* **demo:** fsapi mechanism throws instead of silently falling back to Blob ([eed4fdb](https://github.com/4thel00z/ytdown/commit/eed4fdb8278c7e66e9986406b384a5a42a49c3dc))
* **web:** alias browser-forbidden Origin/Referer headers across SDK and proxy ([b6f846b](https://github.com/4thel00z/ytdown/commit/b6f846b4bcb360aa84661d6ca42ce9b723a9f924))

## [0.3.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.2.0...ytdown-v0.3.0) (2026-06-06)


### Features

* **demo:** self-contained single-file HTML generator with preset + bulk UI ([8a814a4](https://github.com/4thel00z/ytdown/commit/8a814a487834c2aecd047f5d3d9bc7b7d258600d))
* **proxy:** reference Cloudflare Worker passthrough with CORS ([15c12b1](https://github.com/4thel00z/ytdown/commit/15c12b15c65ec186e766965fd72932056593cb60))
* **web:** downloadToDisk with FS Access streaming + Blob fallback ([5064dab](https://github.com/4thel00z/ytdown/commit/5064dab063f6216fa47f9d43b28a8179f712aca4))
* **web:** proxy fetch callback for the WASM transport ([dc32d7b](https://github.com/4thel00z/ytdown/commit/dc32d7bcc6994865b84e7606ffbf78b14a109941))
* **web:** Ytdown wrapper wiring proxy fetch into the wasm core ([c7748c4](https://github.com/4thel00z/ytdown/commit/c7748c4111daac8cbaa999d40a770921d317ede4))


### Bug Fixes

* **demo:** best-audio preset matches Rust (AudioOnly only, no progressive fallback) ([36b09e4](https://github.com/4thel00z/ytdown/commit/36b09e47e9a177b197576d9db963d2e7163e0807))
* **demo:** container ext, preset parity with FormatSelector, blob revoke + FS abort ([88fbfbf](https://github.com/4thel00z/ytdown/commit/88fbfbf1ac4eed2299851e3475b53d8cbd147129))
* **demo:** fsapi mechanism throws instead of silently falling back to Blob ([eed4fdb](https://github.com/4thel00z/ytdown/commit/eed4fdb8278c7e66e9986406b384a5a42a49c3dc))
* **web:** alias browser-forbidden Origin/Referer headers across SDK and proxy ([b6f846b](https://github.com/4thel00z/ytdown/commit/b6f846b4bcb360aa84661d6ca42ce9b723a9f924))

## [0.2.0](https://github.com/4thel00z/ytdown/compare/ytdown-v0.1.0...ytdown-v0.2.0) (2026-06-06)


### Features

* **demo:** self-contained single-file HTML generator with preset + bulk UI ([8a814a4](https://github.com/4thel00z/ytdown/commit/8a814a487834c2aecd047f5d3d9bc7b7d258600d))
* **proxy:** reference Cloudflare Worker passthrough with CORS ([15c12b1](https://github.com/4thel00z/ytdown/commit/15c12b15c65ec186e766965fd72932056593cb60))
* **web:** downloadToDisk with FS Access streaming + Blob fallback ([5064dab](https://github.com/4thel00z/ytdown/commit/5064dab063f6216fa47f9d43b28a8179f712aca4))
* **web:** proxy fetch callback for the WASM transport ([dc32d7b](https://github.com/4thel00z/ytdown/commit/dc32d7bcc6994865b84e7606ffbf78b14a109941))
* **web:** Ytdown wrapper wiring proxy fetch into the wasm core ([c7748c4](https://github.com/4thel00z/ytdown/commit/c7748c4111daac8cbaa999d40a770921d317ede4))


### Bug Fixes

* **demo:** best-audio preset matches Rust (AudioOnly only, no progressive fallback) ([36b09e4](https://github.com/4thel00z/ytdown/commit/36b09e47e9a177b197576d9db963d2e7163e0807))
* **demo:** container ext, preset parity with FormatSelector, blob revoke + FS abort ([88fbfbf](https://github.com/4thel00z/ytdown/commit/88fbfbf1ac4eed2299851e3475b53d8cbd147129))
* **demo:** fsapi mechanism throws instead of silently falling back to Blob ([eed4fdb](https://github.com/4thel00z/ytdown/commit/eed4fdb8278c7e66e9986406b384a5a42a49c3dc))
* **web:** alias browser-forbidden Origin/Referer headers across SDK and proxy ([b6f846b](https://github.com/4thel00z/ytdown/commit/b6f846b4bcb360aa84661d6ca42ce9b723a9f924))
