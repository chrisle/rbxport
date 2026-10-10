# Nix

The repository is a flake that packages rbxport for Nix on `x86_64-linux` only;
there is no aarch64 Linux build to seed from.

## Run without installing

```sh
nix run github:chrisle/rbxport
```

## Install

```sh
nix profile install github:chrisle/rbxport
```

The seed is pushed to `dev` before `main`, so `github:chrisle/rbxport/dev` is
the freshest seed; the default branch lags one release. The app self-updates on
first launch either way.

## Install declaratively

Add the flake as an input (with `inputs.nixpkgs.follows` so there is one
nixpkgs), then reference its package from a module.

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rbxport = {
      url = "github:chrisle/rbxport";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, ... }@inputs:
    nixpkgs.lib.nixosSystem {
      specialArgs = { inherit inputs; };
      modules = [ ./configuration.nix ];
    };
}
```

```nix
# configuration.nix (or a home-manager module)
{ pkgs, inputs, ... }:
{
  environment.systemPackages = [
    inputs.rbxport.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

## How updates work under Nix

Nix owns an immutable seed: the official AppImage stored in the Nix store. On
first launch it is copied to a writable location under your data directory and
run from there, and rbxport's own updater replaces that copy in place as it does
for any other install. The store is never written to.

The version Nix records for the package is the seed it shipped with, not the
version you are running. The app updates itself on launch, so the copy in your
data directory can be newer than the store's label; trust the version shown in
the app rather than `nix profile list`.

## Development

Inside a checkout, `nix develop` opens a shell with the Rust and Node
toolchains and the Tauri system dependencies:

```sh
nix develop
pnpm install
pnpm dev
```

## Removing

`nix profile remove` removes the package but leaves the writable AppImage and
its extraction cache behind. Delete both to reclaim the space:

```sh
rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/rbxport" \
       "${XDG_CACHE_HOME:-$HOME/.cache}/rbxport"
```
