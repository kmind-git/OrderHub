# Test-only certificates. Never use these private keys in a deployed service.
$ErrorActionPreference = 'Stop'
$fixtureDir = Join-Path $PSScriptRoot 'fixtures'
New-Item -ItemType Directory -Force -Path $fixtureDir | Out-Null

function Write-TestCertificate {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [System.Security.Cryptography.AsymmetricAlgorithm] $Key,
        [Parameter(Mandatory)] [datetimeoffset] $NotBefore,
        [Parameter(Mandatory)] [datetimeoffset] $NotAfter,
        [string] $DnsName = 'localhost'
    )

    if ($Key -is [System.Security.Cryptography.ECDsa]) {
        $request = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
            'CN=localhost', $Key, [System.Security.Cryptography.HashAlgorithmName]::SHA256)
    } else {
        $request = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
            'CN=localhost', $Key, [System.Security.Cryptography.HashAlgorithmName]::SHA256,
            [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
    }
    $san = [System.Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
    $san.AddDnsName($DnsName)
    if ($DnsName -eq 'localhost') {
        $san.AddIpAddress([System.Net.IPAddress]::Parse('127.0.0.1'))
    }
    $request.CertificateExtensions.Add($san.Build())
    $request.CertificateExtensions.Add(
        [System.Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new(
            $false, $false, 0, $true))
    $request.CertificateExtensions.Add(
        [System.Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new(
            [System.Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))

    $certificate = $request.CreateSelfSigned($NotBefore, $NotAfter)
    try {
        [System.IO.File]::WriteAllBytes(
            (Join-Path $fixtureDir "$Name.der"),
            $certificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))
        [System.IO.File]::WriteAllBytes(
            (Join-Path $fixtureDir "$Name.pk8"), $Key.ExportPkcs8PrivateKey())
    } finally {
        $certificate.Dispose()
        $Key.Dispose()
    }
}

Write-TestCertificate -Name localhost -Key (
    [System.Security.Cryptography.ECDsa]::Create([System.Security.Cryptography.ECCurve+NamedCurves]::nistP256)
) -NotBefore ([datetimeoffset]'2020-01-01T00:00:00Z') -NotAfter ([datetimeoffset]'2040-01-01T00:00:00Z')

Write-TestCertificate -Name expired -Key (
    [System.Security.Cryptography.ECDsa]::Create([System.Security.Cryptography.ECCurve+NamedCurves]::nistP256)
) -NotBefore ([datetimeoffset]'2010-01-01T00:00:00Z') -NotAfter ([datetimeoffset]'2011-01-01T00:00:00Z')

Write-TestCertificate -Name rsa -Key (
    [System.Security.Cryptography.RSA]::Create(2048)
) -NotBefore ([datetimeoffset]'2020-01-01T00:00:00Z') -NotAfter ([datetimeoffset]'2040-01-01T00:00:00Z')

Write-TestCertificate -Name wronghost -Key (
    [System.Security.Cryptography.ECDsa]::Create([System.Security.Cryptography.ECCurve+NamedCurves]::nistP256)
) -NotBefore ([datetimeoffset]'2020-01-01T00:00:00Z') -NotAfter ([datetimeoffset]'2040-01-01T00:00:00Z') -DnsName wrong.example
