{

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    maelstrom-bin = {
      url = "https://github.com/jepsen-io/maelstrom/releases/download/v0.2.3/maelstrom.tar.bz2";
      flake = false;
    };
    nixpkgs.follows = "rust-overlay/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, maelstrom-bin, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [
          rust-overlay.overlays.default
        ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        #
        # Maelstrom package / test cases
        #
        maelstrom = let
          in pkgs.writeShellScriptBin "maelstrom" ''
            PATH=${pkgs.lib.strings.makeSearchPath "bin"
              [
                pkgs.bash
                pkgs.coreutils #dirname
                pkgs.git

                pkgs.jdk
                pkgs.graphviz
                pkgs.gnuplot
              ]}
            ${maelstrom-bin}/maelstrom $*
          '';
        assert_one_binary_input = ''
          if [ $# -ne 1 ]; then
            echo "USAGE: $(basename $0) BINARY"
            exit 1
          fi
          set -x
        '';
        maelstrom-tests = {
          test-echo = pkgs.writeShellScriptBin "test-echo" ''
            ${assert_one_binary_input}
            ${maelstrom}/bin/maelstrom test -w echo --bin $1 --node-count 1 --time-limit 10
          '';
        };
        maelstrom-tests-values = pkgs.lib.attrValues maelstrom-tests;

        #
        # Rust packages
        #
        rustChannel = "beta";
        rustVersion = "latest";
        rustToolchain = pkgs.rust-bin.${rustChannel}.${rustVersion}.default;

        devShellPackages = [
          rustToolchain
          pkgs.bacon
        ];

      in rec {
        packages = maelstrom-tests // {
          inherit maelstrom;
        };
        devShells.default = pkgs.mkShell {
          packages = devShellPackages ++ maelstrom-tests-values ++ [
            (pkgs.writeShellScriptBin "build-test-echo" ''
              set -x
              cargo b && test-echo target/debug/echo
            '')
          ];
          shellHook = ''
            # temporarily enable sparse-index, until stabilized (in rust 1.70?)
            export CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
          '';
        };
      });
}
