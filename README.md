# altreg

A straightforward and full-featured alternate registry implementation for Cargo that does not require a git server.

Uses the nightly `-Z sparse-registry` feature to allow the index to be served over HTTP, and so you must be running nightly Cargo to use this registry.

_Name is a working title and is subject to change._

## Features
- Host local crates
- Cache crates.io index files and crate files
- Web UI that displays crates
- Render local crates' readmes

## Roadmap
### 0.1.0
- [x] crates.io passthrough for all requests
### 0.2.0
- [x] Hot caching of upstream .crate files
- [x] Hot caching of upstream index files
- [x] Uploading of crates
### 0.3.0
- [x] Web UI
  - [x] Render crate readmes
  - [x] Build and display crate docs
  - [x] Search crates
### 0.4.0
- [ ] Authentication
- [ ] Authorisation
### 0.5.0
- [ ] Tool to provide full mirrors of upstream for offline caches
- [ ] Index base inheritance
  - [ ] Upstreams other than crates.io
### 1.0.0
- [ ] Stabilisation of API
- [ ] Production hardening
- [ ] Publish this crate to crates.io

## Installation

```
> git clone https://github.com/calebfletcher/altreg.git
> cargo install --path altreg
```

_COMING SOON_
~```> cargo install altreg```~

## Usage
Run registry:

```
> altreg
```

Add registry to Cargo's config by putting the following into either your global `~/.cargo/config.toml` or your project's `.cargo/config.toml`:
```
[registries.private]
index = "http://localhost:1491"
```
This assumes you want the registry to be named `private` and is running on your local machine on port 1491. Update these as required.

If you would like to use this registry as the default, add this to Cargo's config (updating the registry's name where appropriate):
```
[registry]
default = "private"
```
