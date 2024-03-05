{ inputs, ... }: {
  perSystem = { pkgs, config, ... }: {
    treefmt.config = {
      projectRootFile = "flake.nix";
      programs = {
        rustfmt.enable = true;
        nixfmt.enable = true;
        prettier.enable = true;
      };
    };
  };
}
