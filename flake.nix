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
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [
        rust-overlay.overlays.default
      ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      # Nix
      nix-alejandra = let
        has_extension = ext: path: (! builtins.isNull builtins.match ".*\\.${ext}" path);
      in
        pkgs.stdenvNoCC.mkDerivation {
          name = "nix-alejandra-check";
          srcs =
            builtins.filterSource
            (path: type: type != "file" || has_extension "nix" path)
            ./.;
          phases = ["buildPhase"];
          buildPhase = ''
            ${pkgs.alejandra}/bin/alejandra -c $srcs
            touch $out
          '';
        };

      #
      # Maelstrom package / test cases
      #
      maelstrom = pkgs.writeShellScriptBin "maelstrom" ''
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
        maelstrom-test-unique = pkgs.writeShellScriptBin "test-unique" ''
          ${assert_one_binary_input}
          ${maelstrom}/bin/maelstrom test -w unique-ids --bin $1 --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition
        '';
        maelstrom-test-broadcast-single = pkgs.writeShellScriptBin "test-broadcast-single" ''
          ${assert_one_binary_input}
          ${maelstrom}/bin/maelstrom test -w broadcast --bin $1 --node-count 1 --time-limit 20 --rate 10
        '';
        maelstrom-test-broadcast-connected = pkgs.writeShellScriptBin "test-broadcast-connected" ''
          ${assert_one_binary_input}
          ${maelstrom}/bin/maelstrom test -w broadcast --bin $1 --node-count 5 --time-limit 20 --rate 10
        '';
        maelstrom-test-broadcast = pkgs.writeShellScriptBin "test-broadcast" ''
          ${assert_one_binary_input}
          ${maelstrom}/bin/maelstrom test -w broadcast --bin $1 --node-count 5 --time-limit 20 --rate 10 --nemesis partition
        '';
        maelstrom-test-broadcast-stress = let
          numeric_check_lt = {
            value,
            threshold,
            label,
            units,
          }: ''
            if (( $(echo "${value} > ${threshold}" | ${pkgs.bc}/bin/bc -l) )); then
              echo "${label} is out of range: ${value} (should be within ${threshold} ${units})"
              FAIL=1
            fi
          '';
        in
          pkgs.writeShellScriptBin "test-broadcast-stress" ''
            # exit on first error
            set -e
            ${assert_one_binary_input}
            ${maelstrom}/bin/maelstrom test -w broadcast --bin $1 --node-count 25 --time-limit 20 --rate 100 --latency 100 | tee tmp.out

            # analysis
            grep "msgs-per-op" tmp.out
            grep stable-latencies tmp.out -A 4

            set +x
            msgs_per_op1=$(grep "msgs-per-op" tmp.out | cut -d "p" -f3 | cut -d "}" -f1 | xargs echo | cut -d " " -f 1)
            msgs_per_op2=$(grep "msgs-per-op" tmp.out | cut -d "p" -f3 | cut -d "}" -f1 | xargs echo | cut -d " " -f 2)
            latency_median=$(grep stable-latencies tmp.out -A 4 | grep "0.5 " | xargs echo | cut -d " " -f 2- | cut -d "," -f1)
            latency_max=$(grep stable-latencies tmp.out -A 4 | grep "1 " | xargs echo | cut -d " " -f 2- | cut -d "}" -f1)

            FAIL=0
            ${numeric_check_lt {
              value = "$msgs_per_op1";
              threshold = "30";
              label = "Messages per op (#1)";
              units = "messages per op";
            }}
            ${numeric_check_lt {
              value = "$msgs_per_op2";
              threshold = "30";
              label = "Messages per op (#2)";
              units = "messages per op";
            }}
            ${numeric_check_lt {
              value = "$latency_median";
              threshold = "400";
              label = "Latency Median";
              units = "ms";
            }}
            ${numeric_check_lt {
              value = "$latency_max";
              threshold = "600";
              label = "Latency Max";
              units = "ms";
            }}

            if [ $FAIL -eq 0 ]; then
              echo "Output analysis check passed."
              rm tmp.out
            else
              echo "Output analysis check failed, persisting output file tmp.out"
            fi

            exit $FAIL
          '';
      };
      maelstrom-tests-values = pkgs.lib.attrValues maelstrom-tests;
      maelstrom-regression = pkgs.writeShellScriptBin "maelstrom-regression" ''
        # exit on first error
        set -e

        ${maelstrom-tests.maelstrom-test-echo}/bin/test-echo ${crate.package}/bin/echo

        ${maelstrom-tests.maelstrom-test-unique}/bin/test-unique ${crate.package}/bin/unique

        # NOTE: These are now redundant, see below
        # ${maelstrom-tests.maelstrom-test-broadcast-single}/bin/test-broadcast-single ${crate.package}/bin/broadcast
        # ${maelstrom-tests.maelstrom-test-broadcast-connected}/bin/test-broadcast-connected ${crate.package}/bin/broadcast
        # ${maelstrom-tests.maelstrom-test-broadcast}/bin/test-broadcast ${crate.package}/bin/broadcast
        ${maelstrom-tests.maelstrom-test-broadcast-stress}/bin/test-broadcast-stress ${crate.package}/bin/broadcast

      '';

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
        # other
        maelstrom
      ];

      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
      crate = pkgs.callPackage ./crate.nix {
        inherit advisory-db system craneLib;
      };
    in {
      checks =
        crate.checks
        // {
          inherit nix-alejandra;
        };

      devShells.default = pkgs.mkShell {
        packages =
          devShellPackages
          ++ maelstrom-tests-values
          ++ [
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

      packages =
        maelstrom-tests
        // {
          inherit maelstrom;
          tests = maelstrom-regression;
          test-broadcast-stress = pkgs.writeShellScriptBin "test-broadcast-stress" ''
            ${maelstrom-tests.maelstrom-test-broadcast-stress}/bin/test-broadcast-stress ${crate.package}/bin/broadcast
          '';
          default = crate.package;
        };
    });
}
