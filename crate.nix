{ pkgs, system, craneLib, advisory-db, extraBuildArgs ? {}, srcDir ? ./. }:
let
  src = craneLib.cleanCargoSource srcDir;

  # Common arguments can be set here to avoid repeating them later
  commonArgs = {
    inherit src;

    buildInputs = [
      # Add additional build inputs here
    ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      # Additional darwin specific inputs can be set here
      pkgs.libiconv
    ];
  };

  # Build *just* the cargo dependencies, so we can reuse
  # all of that work (e.g. via cachix) when running in CI
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # Build the actual crate itself, reusing the dependency
  # artifacts from above.
  my-crate = craneLib.buildPackage (commonArgs // {
    inherit cargoArtifacts;
  } // extraBuildArgs);
in {
  checks = {
    # Build the crate as part of `nix flake check` for convenience
    inherit my-crate;

    # Run clippy (and deny all warnings) on the crate source,
    # again, resuing the dependency artifacts from above.
    #
    # Note that this is done as a separate derivation so that
    # we can block the CI if there are issues here, but not
    # prevent downstream consumers from building our crate by itself.
    my-crate-clippy = craneLib.cargoClippy (commonArgs // {
      inherit cargoArtifacts;
      cargoClippyExtraArgs = "--all-targets -- --deny warnings";
    });

    my-crate-doc = craneLib.cargoDoc (commonArgs // {
      inherit cargoArtifacts;
    });

    # Check formatting
    my-crate-fmt = craneLib.cargoFmt {
      inherit src;
    };

    # Audit dependencies
    my-crate-audit = craneLib.cargoAudit {
      inherit src advisory-db;
    };

    # Run tests with cargo-nextest
    # Consider setting `doCheck = false` on `my-crate` if you do not want
    # the tests to run twice
    my-crate-nextest = craneLib.cargoNextest (commonArgs // {
      inherit cargoArtifacts;
      partitions = 1;
      partitionType = "count";
  # TODO: enable code coverage, only if it's worth it
  # } // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
  #   # NB: cargo-tarpaulin only supports x86_64 systems
  #   # Check code coverage (note: this will not upload coverage anywhere)
  #   my-crate-coverage = craneLib.cargoTarpaulin (commonArgs // {
  #     inherit cargoArtifacts;
  #   });
    });
  };

  package = my-crate;
}
