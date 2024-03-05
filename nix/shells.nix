{ ... }: {
  perSystem = { config, pkgs, ... }: {
    devShells.default = config.nci.outputs.kairos-trie.devShell;
  };
}
