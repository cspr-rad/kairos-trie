{ inputs, ... }: {
  perSystem = { pkgs, config, ... }: {
    nci = {
      projects.kairos-trie = {
        path = ./../.;
        export = true;
      };
      crates.kairos-trie = { };
    };
  };
}
