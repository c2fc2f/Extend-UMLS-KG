{
  description = "A multitool for extending UMLS knowledge graphs (CSV-based for Neo4J) with additional nodes, relationships, and external metadata";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default-linux";
  };

  outputs =
    {
      self,
      nixpkgs,
      systems,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      eachSystem = lib.genAttrs (import systems);

      pkgsFor = eachSystem (
        system:
        import nixpkgs {
          localSystem = system;
        }
      );
    in
    {
      packages = eachSystem (
        system: 
        {
          default = self.packages.${system}.xumlskg;

          xumlskg = pkgsFor.${system}.callPackage ./nix/package.nix {
            version = self.rev or self.dirtyRev or "dirty";
          };
        }
      );

      devShells = eachSystem (system: {
        default =
          pkgsFor.${system}.mkShell.override
            {
              inherit (self.packages.${system}.default) stdenv;
            }
            {
              env = {
                # Required by rust-analyzer
                RUST_SRC_PATH = "${pkgsFor.${system}.rustPlatform.rustLibSrc}";
              };

              nativeBuildInputs = with pkgsFor.${system}; [
                cargo
                rustc
                rust-analyzer
                rustfmt
                clippy

                rustPlatform.bindgenHook
              ];

              buildInputs = [ ];
            };
      });
    };
}
