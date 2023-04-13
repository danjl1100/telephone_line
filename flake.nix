{

  inputs = {
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.flake-utils.follows = "flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
    };
    flake-utils.url = "github:numtide/flake-utils";
    maelstrom-bin = {
      url = "https://github.com/jepsen-io/maelstrom/releases/download/v0.2.3/maelstrom.tar.bz2";
      flake = false;
    };
    nixpkgs.follows = "rust-overlay/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
      self,
      advisory-db,
      crane,
      flake-utils,
      maelstrom-bin,
      nixpkgs,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [
          rust-overlay.overlays.default
        ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Nix
        nix-alejandra = let
          has_extension = ext: path: (! builtins.isNull builtins.match ".*\\.${ext}" path);
        in pkgs.stdenvNoCC.mkDerivation {
          name = "nix-alejandra-check";
          srcs = builtins.filterSource
            (path: type: type != "file" || has_extension "nix" path)
            ./.
            ;
          phases = ["buildPhase"];
          buildPhase = ''
            ${pkgs.alejandra}/bin/alejandra -c $srcs
          '';
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
          maelstrom-test-echo = pkgs.writeShellScriptBin "test-echo" ''
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
          # Nix
          pkgs.alejandra
          # Rust
          rustToolchain
          pkgs.bacon
        ];


        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        crate = pkgs.callPackage ./crate.nix {
          inherit advisory-db system craneLib;
        };

      in {
        checks = crate.checks // {
          inherit nix-alejandra;
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

        packages = maelstrom-tests // {
          inherit maelstrom;
          default = crate.package;
        };

      });
}
