{
  description = "srv-rs-upd";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1.590113.tar.gz";

    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1.1584.tar.gz";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-compat.url = "https://flakehub.com/f/edolstra/flake-compat/1.0.0.tar.gz";
  };

  outputs =
    { self
    , nixpkgs
    , fenix
    , naersk
    , ...
    } @ inputs:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: (forSystem system f));

      forSystem = system: f: f rec {
        inherit system;
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
      };

      fenixToolchain = system: with fenix.packages.${system};
        combine ([
          stable.clippy
          stable.rustc
          stable.cargo
          stable.rustfmt
          stable.rust-src
          stable.rust-analyzer
        ] ++ nixpkgs.lib.optionals (system == "x86_64-linux") [
          targets.x86_64-unknown-linux-musl.stable.rust-std
        ] ++ nixpkgs.lib.optionals (system == "aarch64-linux") [
          targets.aarch64-unknown-linux-musl.stable.rust-std
        ]);
    in
    {
      devShells = forAllSystems ({ system, pkgs, ... }:
        let
          toolchain = fenixToolchain system;
        in {
          default = pkgs.mkShell {
            name = "srv-rs-upd-shell";

            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";

            nativeBuildInputs = with pkgs; [ ];
            buildInputs = with pkgs; [
              toolchain
              cargo-outdated
              cacert
              cargo-audit
              cargo-watch
              cargo-readme
              cargo-nextest
              nixpkgs-fmt
              libiconv
            ];
          };
        });

      packages = forAllSystems ({ system, pkgs, ... }: {
        default = pkgs.srv-rs-upd;
      });
    };
}
