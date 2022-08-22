# altreg

A straightforward and full-featured alternate registry implementation for Cargo that does not require a git server.

Uses the nightly `-Z sparse-registry` feature to allow the index to be served over HTTP, and so you must be running nightly Cargo to use this registry.

_Name is a working title and is subject to change._

## Roadmap
### 0.1.0
- [x] crates.io passthrough for all requests
### 0.2.0
- [ ] Hot caching of upstream .crate files
- [ ] Hot caching of upstream index files
- [ ] Tool to provide full mirrors of upstream for offline caches
### 0.3.0
- [ ] Index base inheritance
  - [ ] Upstreams other than crates.io
- [ ] Uploading of crates
- [ ] Publish this crate to crates.io
### 0.4.0
- [ ] Web UI
  - [ ] View indexes
  - [ ] Search crates
### 0.5.0
- [ ] Authentication
- [ ] Authorisation
### 1.0.0
- [ ] Stabilisation of API
- [ ] Production hardening

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
index = "http://localhost:3000"
```
This assumes you want the registry to be named `private` and is running on your local machine on port 3000. Update these as required.

If you would like to use this registry as the default, add this to Cargo's config (updating the registry's name where appropriate):
```
[registry]
default = "private"
```
