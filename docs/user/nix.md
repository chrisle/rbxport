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

The pin is pushed to `dev` before `main`, so `github:chrisle/rbxport/dev` has
the newest release first; the default branch lags one release.

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

Nix owns a Nix install, including its updates. The package runs the official
AppImage straight from the read-only Nix store, and rbxport's built-in updater
is unavailable: it does not check the download server, download, or replace
anything. Check for updates in the app says the copy was installed with Nix
and that updates come through your Nix channel.

To update, move your flake input or profile to the newer release, for example:

```sh
nix profile upgrade <name>   # an imperative install; `nix profile list` shows the name
nix flake update rbxport     # a system flake that lists rbxport as an input, then rebuild
```

The version Nix records for the package is the version you run.

Earlier Nix packages copied the AppImage into
`${XDG_DATA_HOME:-$HOME/.local/share}/rbxport` and let the built-in updater
replace that copy. The current launcher removes that copy on its first run, so
the app you run is the one Nix installed, which can be older than the copy the
updater had reached until your Nix channel catches up. A copy that an earlier
launcher still starts recognises the copy it seeded and does not update itself
either.

## Development

Inside a checkout, `nix develop` opens a shell with the Rust and Node
toolchains and the Tauri system dependencies:

```sh
nix develop
pnpm install
pnpm dev
```

## Removing

`nix profile remove` removes the package but leaves its extraction cache (and,
from an earlier package, a copied AppImage) behind. Delete both to reclaim the
space:

```sh
rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/rbxport" \
       "${XDG_CACHE_HOME:-$HOME/.cache}/rbxport"
```
