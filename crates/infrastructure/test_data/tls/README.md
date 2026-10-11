# Local TLS fixtures

These certificate/key bytes were copied without modification from the accepted P1 fixtures
in `prototypes/orderhub-no-c-tls/fixtures`. The corresponding PowerShell generator uses the
platform certificate APIs; tests read fixed files and require no generator or OpenSSL.
The keys are public test material. They must never be used for a deployed identity.

The cases cover localhost ECDSA/RSA, wrong hostname and expired certificates.
Positive fixtures have a fixed validity interval; expiration is an intentional future test failure.
