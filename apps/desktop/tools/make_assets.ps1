# The app's icon and the logo in its side bar, from media/kebitx.png. Run it again
# only when the logo changes; what it writes is kept in src/.
#
#   powershell -File apps/desktop/tools/make_assets.ps1
Add-Type -AssemblyName System.Drawing
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..\..')
$source = [System.Drawing.Bitmap]::FromFile((Join-Path $root 'media\kebitx.png'))
$out = Join-Path $root 'apps\desktop\src'

# the logo with its orbit, and for tiny sizes the cube alone: at 16 pixels an orbit
# is a smudge
$whole = New-Object System.Drawing.Rectangle(270, 40, 1000, 1000)
$cube = New-Object System.Drawing.Rectangle(470, 150, 600, 600)

function Render($size, $crop, $round) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    if ($round) {
        # a dark tile, so it reads on a light taskbar as well as on a dark one
        $r = [Math]::Max(2, [int]($size * 0.2)); $d = $r * 2; $s = $size - 1
        $path = New-Object System.Drawing.Drawing2D.GraphicsPath
        $path.AddArc(0, 0, $d, $d, 180, 90); $path.AddArc($s - $d, 0, $d, $d, 270, 90)
        $path.AddArc($s - $d, $s - $d, $d, $d, 0, 90); $path.AddArc(0, $s - $d, $d, $d, 90, 90)
        $path.CloseFigure()
        $g.SetClip($path)
    }
    $target = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
    $g.DrawImage($source, $target, $crop, [System.Drawing.GraphicsUnit]::Pixel)
    $g.Dispose()
    return $bitmap
}

function PixelBytes($bitmap) {
    $size = $bitmap.Width
    $bytes = New-Object byte[] ($size * $size * 4)
    $at = 0
    for ($y = 0; $y -lt $size; $y++) {
        for ($x = 0; $x -lt $size; $x++) {
            $c = $bitmap.GetPixel($x, $y)
            $bytes[$at] = $c.B; $bytes[$at + 1] = $c.G; $bytes[$at + 2] = $c.R; $bytes[$at + 3] = $c.A
            $at += 4
        }
    }
    return ,$bytes
}

# for the side bar: raw pixels, top-down; the app rounds the corners itself
$logo = Render 96 $whole $false
[System.IO.File]::WriteAllBytes((Join-Path $out 'logo96.bgra'), (PixelBytes $logo))
$logo.Dispose()

# the .ico
$images = New-Object System.Collections.ArrayList
foreach ($size in 16, 24, 32, 48, 64, 256) {
    $bitmap = Render $size $(if ($size -le 32) { $cube } else { $whole }) $true
    $stream = New-Object System.IO.MemoryStream
    if ($size -eq 256) {
        $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        $w = New-Object System.IO.BinaryWriter($stream)
        $w.Write([int]40); $w.Write([int]$size); $w.Write([int]($size * 2))
        $w.Write([int16]1); $w.Write([int16]32)
        foreach ($i in 1..6) { $w.Write([int]0) }
        $pixels = PixelBytes $bitmap
        # bottom-up in an .ico
        for ($y = $size - 1; $y -ge 0; $y--) { $w.Write($pixels, $y * $size * 4, $size * 4) }
        $w.Write((New-Object byte[] ([int][Math]::Floor(($size + 31) / 32) * 4 * $size)))
        $w.Flush()
    }
    $bitmap.Dispose()
    [void]$images.Add(@($size, $stream.ToArray()))
}
$file = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter($file)
$w.Write([int16]0); $w.Write([int16]1); $w.Write([int16]$images.Count)
$offset = 6 + 16 * $images.Count
foreach ($image in $images) {
    $s = if ($image[0] -eq 256) { 0 } else { $image[0] }
    $w.Write([byte]$s); $w.Write([byte]$s); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([int16]1); $w.Write([int16]32)
    $w.Write([int]$image[1].Length); $w.Write([int]$offset)
    $offset += $image[1].Length
}
foreach ($image in $images) { $w.Write($image[1]) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $out 'app.ico'), $file.ToArray())
$source.Dispose()
"icon: $($file.Length) bytes"
