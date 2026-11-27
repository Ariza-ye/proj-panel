param(
    [Parameter(Mandatory = $true)]
    [string]$InputPng,

    [Parameter(Mandatory = $true)]
    [string]$OutputIco
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

$sizes = @(16, 32, 48, 256)
$pngBlobs = New-Object System.Collections.Generic.List[byte[]]

$source = [System.Drawing.Image]::FromFile((Resolve-Path $InputPng).Path)
try {
    foreach ($sz in $sizes) {
        $bmp = New-Object System.Drawing.Bitmap($sz, $sz, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            try {
                $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
                $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $g.Clear([System.Drawing.Color]::Transparent)
                $g.DrawImage($source, 0, 0, $sz, $sz)
            } finally {
                $g.Dispose()
            }

            $ms = New-Object System.IO.MemoryStream
            try {
                $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
                $pngBlobs.Add($ms.ToArray())
            } finally {
                $ms.Dispose()
            }
        } finally {
            $bmp.Dispose()
        }
    }
} finally {
    $source.Dispose()
}

$outDir = Split-Path -Parent $OutputIco
if ($outDir -and -not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

# Write ICO header (6 bytes) + directory entries (16 bytes each) + PNG payloads.
$fs = [System.IO.File]::Open($OutputIco, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
$bw = New-Object System.IO.BinaryWriter($fs)
try {
    $bw.Write([UInt16]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]$sizes.Count)

    $offset = 6 + (16 * $sizes.Count)
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $sz = $sizes[$i]
        $dimByte = if ($sz -ge 256) { [byte]0 } else { [byte]$sz }
        $blob = $pngBlobs[$i]

        $bw.Write($dimByte)
        $bw.Write($dimByte)
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([UInt16]1)
        $bw.Write([UInt16]32)
        $bw.Write([UInt32]$blob.Length)
        $bw.Write([UInt32]$offset)
        $offset += $blob.Length
    }

    foreach ($blob in $pngBlobs) {
        $bw.Write($blob)
    }
} finally {
    $bw.Dispose()
    $fs.Dispose()
}

Write-Host "Generated Windows icon: $OutputIco"
