{
  description = "kairos-trie";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    nci = {
      url = "github:yusdacra/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    parts.url = "github:hercules-ci/flake-parts";
    fmt = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = inputs@{ self, nixpkgs, nci, parts, fmt }:
    parts.lib.mkFlake { inherit inputs; } {
      flake.herculesCI.ciSystems = [ "x86_64-linux" ];
      systems = [ "x86_64-linux" "aarch64-darwin" ];
      imports = [
        nci.flakeModule
        ./nix/crates.nix
        ./nix/shells.nix
        fmt.flakeModule
        ./nix/format.nix
      ];
    };
}
