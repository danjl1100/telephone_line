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
      maelstrom = pkgs.stdenvNoCC.mkDerivation {
        name = "maelstrom";
        src = maelstrom-bin;
        buildPhase = ''
          mkdir -p target
          cp -r $src/lib target/
          cp $src/maelstrom target/maelstrom-bin
          echo "
          PATH=${pkgs.lib.strings.makeSearchPath "bin"
            [
              pkgs.bash
              pkgs.coreutils #dirname
              pkgs.git

              pkgs.jdk
              pkgs.graphviz
              pkgs.gnuplot
            ]}
          $out/bin/maelstrom-bin \$*
          " > target/maelstrom
          chmod +x target/maelstrom
        '';
        installPhase = ''
          mkdir -p $out
          cp -r target $out/bin
        '';
      };

      maelstrom-cases = rec {
        echo = {
          bin = "echo";
          maelstrom-args = [
            "-w echo"
            "--node-count 1"
            "--time-limit 10"
          ];
        };
        unique = {
          bin = "unique";
          maelstrom-args = [
            "-w unique-ids"
            "--time-limit 30"
            "--rate 1000"
            "--node-count 3"
            "--availability total"
            "--nemesis partition"
          ];
        };
        broadcast-single = {
          bin = "broadcast";
          maelstrom-args = [
            "-w broadcast"
            "--node-count 1"
            "--time-limit 20"
            "--rate 10"
          ];
        };
        broadcast-connected = {
          bin = "broadcast";
          maelstrom-args = [
            "-w broadcast"
            "--node-count 5"
            "--time-limit 20"
            "--rate 10"
          ];
        };
        broadcast = {
          bin = "broadcast";
          maelstrom-args = [
            "-w broadcast"
            "--node-count 5"
            "--time-limit 20"
            "--rate 10"
            "--nemesis partition"
          ];
        };
        broadcast-stress = {
          inherit (broadcast) bin;
          maelstrom-args = [
            "-w broadcast"
            "--node-count 25"
            "--time-limit 20"
            "--rate 100"
            "--latency 100"
          ];
          analysis-params = {
            msgs-per-op = 21;
            latency-median = 400;
            latency-max = 650; # difficult to perfectly achieve both goals simultaneously, so just nudge this one a bit
          };
        };
        broadcast-stress-low-latency = {
          inherit (broadcast-stress) bin maelstrom-args;
          bin-args = ["--low-latency"];
          analysis-params = {
            msgs-per-op = 30;
            latency-median = 400;
            latency-max = 600;
          };
        };
        broadcast-stress-low-bandwidth = {
          inherit (broadcast-stress) bin maelstrom-args;
          bin-args = ["--low-bandwidth"];
          analysis-params = {
            msgs-per-op = 20;
            latency-median = 1000;
            latency-max = 2000;
          };
        };
        counter = {
          bin = "counter";
          maelstrom-args = [
            "-w g-counter"
            "--node-count 3"
            "--rate 100"
            "--time-limit 20"
            "--nemesis partition"
          ];
        };
        logs-single = {
          bin = "logs";
          maelstrom-args = [
            "-w kafka"
            "--node-count 1"
            "--concurrency 2n"
            "--time-limit 20"
            "--rate 1000"
          ];
        };
        logs = {
          bin = "logs";
          maelstrom-args = [
            "-w kafka"
            "--node-count 2"
            "--concurrency 2n"
            "--time-limit 20"
            "--rate 1000"
          ];
          regression-ignore = true;
        };
      };
      maelstrom-script = label: {
        bin,
        bin-args ? [],
        maelstrom-args,
        analysis-params ? null,
        regression-ignore ? false, # unused
      }: let
        out-file = ".test-${label}.out";
        exit-code-var = "FAIL";
      in
        pkgs.writeShellScriptBin "test-${label}" ''
          set -x
          cargo build --bin ${pkgs.lib.escapeShellArg bin} \
          && \
            ${maelstrom}/bin/maelstrom test \
            ${pkgs.lib.escapeShellArgs maelstrom-args} \
            --bin ''${1:-target/debug/${bin}} \
            -- ${pkgs.lib.escapeShellArgs bin-args} ${
            if isNull analysis-params
            then ""
            else ''
              | tee ${pkgs.lib.escapeShellArg out-file}
              ${exit-code-var}=0
              ${maelstrom-analysis-text
                {
                  inherit label exit-code-var;
                  src = pkgs.lib.escapeShellArg out-file;
                }
                analysis-params}
              if [ ''$${exit-code-var} -eq 0 ]; then
                  rm ${pkgs.lib.escapeShellArg out-file}
              fi
            ''
          }
        '';
      maelstrom-derivation = label: {
        bin,
        bin-args ? [],
        maelstrom-args,
        analysis-params ? null,
        regression-ignore ? false, # unused
      }: let
        out-file = pkgs.stdenvNoCC.mkDerivation {
          name = "test-${label}";
          phases = ["buildPhase" "installPhase"];
          nativeBuildInputs = [
            maelstrom
          ];
          buildPhase = ''
            maelstrom test \
              ${pkgs.lib.escapeShellArgs maelstrom-args} \
              --bin "${crate.package}/bin/${bin}" \
              -- ${pkgs.lib.escapeShellArgs bin-args} \
              | tee .test-${label}.out
            # remove extraneous symlinks
            rm store/current
            rm store/latest
            find store -name 'latest' -exec rm {} \;
          '';
          installPhase = ''
            mkdir -p $out
            cp -r store $out/
            cp .test-${label}.out $out/
          '';
        };
      in (
        if isNull analysis-params
        then out-file
        else
          (pkgs.stdenvNoCC.mkDerivation {
            name = "analysis-${label}";
            src = "${out-file}/.test-${label}.out";
            phases = ["buildPhase"];
            buildPhase = let
              exit-code-var = "FAIL";
            in ''
              ${maelstrom-analysis-text {
                  inherit label exit-code-var;
                  src = "$src";
                }
                analysis-params}
              if [ ''$${exit-code-var} -eq 0 ]; then
                mkdir $out
              fi
              exit ''$${exit-code-var}
            '';
          })
      );
      maelstrom-analysis-text = {
        label,
        src,
        exit-code-var,
      }: {
        msgs-per-op,
        latency-median,
        latency-max,
      }: let
        numeric_check_lt = {
          value,
          threshold,
          label,
          units,
        }: ''
          if (( $(echo "${value} < ${toString threshold}" | ${pkgs.bc}/bin/bc -l) )); then
            echo "[PASS] ${label}: ${value} (below goal of ${toString threshold} ${units})"
          else
            echo "[FAIL] ${label} is out of range: ${value} (should be below goal of ${toString threshold} ${units})"
            ${exit-code-var}=1
          fi
        '';
      in ''
        # exit on first error
        set -e

        # print analysis
        grep "msgs-per-op" ${src}
        grep stable-latencies ${src} -A 4

        set +x
        msgs_per_op1=$(grep "msgs-per-op" ${src} | cut -d "p" -f3 | cut -d "}" -f1 | xargs echo | cut -d " " -f 1)
        msgs_per_op2=$(grep "msgs-per-op" ${src} | cut -d "p" -f3 | cut -d "}" -f1 | xargs echo | cut -d " " -f 2)
        latency_median=$(grep stable-latencies ${src} -A 4 | grep "0.5 " | xargs echo | cut -d " " -f 2- | cut -d "," -f1)
        latency_max=$(grep stable-latencies ${src} -A 4 | grep "1 " | xargs echo | cut -d " " -f 2- | cut -d "}" -f1)

        ${exit-code-var}=0
        ${numeric_check_lt {
          value = "$msgs_per_op1";
          threshold = msgs-per-op;
          label = "Messages per op (#1)";
          units = "messages per op";
        }}
        ${numeric_check_lt {
          value = "$msgs_per_op2";
          threshold = msgs-per-op;
          label = "Messages per op (#2)";
          units = "messages per op";
        }}
        ${numeric_check_lt {
          value = "$latency_median";
          threshold = latency-median;
          label = "Latency Median";
          units = "ms";
        }}
        ${numeric_check_lt {
          value = "$latency_max";
          threshold = latency-max;
          label = "Latency Max";
          units = "ms";
        }}

        if [ ''$${exit-code-var} -eq 0 ]; then
          echo "Output analysis check passed."
        else
          echo "Output analysis check failed, reference output file:"
          echo -e "\t" ${src}
        fi
      '';
      maelstrom-test-derivations =
        pkgs.lib.mapAttrs (
          label: case @ {
            bin,
            maelstrom-args,
            bin-args ? [],
            analysis-params ? null,
            regression-ignore ? false,
          }:
            if regression-ignore
            then (pkgs.writeTextDir "store/ignored/${label}.txt" "regression-ignore = true; for test ${label}")
            else (maelstrom-derivation "${label}" case)
        )
        maelstrom-cases;
      maelstrom-test-scripts =
        pkgs.lib.mapAttrs' (
          label: case @ {
            bin,
            maelstrom-args,
            bin-args ? [],
            analysis-params ? null,
            regression-ignore ? false, # unused
          }:
            pkgs.lib.nameValuePair "maelstrom-test-${label}"
            (maelstrom-script "${label}" case)
        )
        maelstrom-cases;

      regression-tests = pkgs.symlinkJoin {
        name = "regression-tests";
        paths =
          (pkgs.lib.attrValues maelstrom-test-derivations)
          ++ [
            (pkgs.writeShellScriptBin "regression-tests" ''
              echo "[PASS] See maelstrom results store at:"
              echo -e "\t$(dirname $(dirname $0))/store"
            '')
          ];
      };

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
          ++ (pkgs.lib.attrValues maelstrom-test-scripts)
          ++ [
            (pkgs.writeShellScriptBin "build-test-echo" ''
              set -x
              cargo b && test-echo target/debug/echo
            '')
          ];
      };

      packages =
        maelstrom-test-scripts
        // {
          inherit maelstrom;
          test-echo = maelstrom-test-derivations.echo;
          test-unique = maelstrom-test-derivations.unique;
          tests = regression-tests;
          test-broadcast-stress = pkgs.writeShellScriptBin "test-broadcast-stress" ''
            # exit on first error
            set -e
            ${maelstrom-test-scripts.maelstrom-test-broadcast-stress-low-latency}/bin/test-broadcast-stress ${crate.package}/bin/broadcast
            ${maelstrom-test-scripts.maelstrom-test-broadcast-stress-low-bandwidth}/bin/test-broadcast-stress ${crate.package}/bin/broadcast
          '';
          serve = pkgs.writeShellScriptBin "maelstrom-serve-tests" ''
            SCRATCH=$(mktemp -d --suffix=regression-tests)
            cp -r -L "${regression-tests}/store" "$SCRATCH"
            chmod -R +w "$SCRATCH"
            trap 'rm -r "$SCRATCH"' EXIT
            echo "Setup scratch dir $SCRATCH"
            cd "$SCRATCH"
            ${maelstrom}/bin/maelstrom serve
          '';
          default = crate.package;
        };
    });
}
