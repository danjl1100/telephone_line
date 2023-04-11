{

  inputs.nixpkgs.url = "nixpkgs"; # "github:NixOS/nixpkgs";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  inputs.maelstrom_bin = {
    url = "https://github.com/jepsen-io/maelstrom/releases/download/v0.2.3/maelstrom.tar.bz2";
    flake = false;
  };

  outputs = { self, nixpkgs, flake-utils, maelstrom_bin }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        deps = [
          pkgs.bash
          pkgs.coreutils #dirname
          pkgs.git

          pkgs.jdk
          pkgs.graphviz
          pkgs.gnuplot
        ];
        assert_one_binary_input = ''
          if [ $# -ne 1 ]; then
            echo "USAGE: $0 BINARY"
            exit 1
          fi
          set -x
        '';
      in {
        packages = rec {
          maelstrom = pkgs.writeShellScriptBin "maelstrom" ''
            PATH=${pkgs.lib.strings.makeSearchPath "bin" deps}
            ${maelstrom_bin}/maelstrom $*
          '';
          test-echo = pkgs.writeShellScriptBin "test-echo" ''
            ${assert_one_binary_input}
            ${maelstrom}/bin/maelstrom test -w echo --bin $1 --node-count 1 --time-limit 10
          '';
        };
        devShells.default = pkgs.mkShell {
          packages = deps;
        };
      });
}
