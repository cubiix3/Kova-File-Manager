# Rebuild Kova's original vector mark, preview and multi-resolution Windows icon.
# Uses only Windows System.Drawing; no external image tools are required.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assetPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'apps/kova-desktop/assets'
New-Item -ItemType Directory -Force -Path $assetPath | Out-Null
$svg = @'
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
  <title>Kova</title>
  <rect x="8" y="8" width="240" height="240" rx="52" fill="#182630"/>
  <path d="M62 58H98V119L157 58H202L134 127L207 198H158L98 139V198H62Z" fill="#86d5f4"/>
  <path d="M134 127L207 198H158L98 139Z" fill="#499dcc"/>
</svg>
'@
[IO.File]::WriteAllText((Join-Path $assetPath 'kova.svg'), $svg + "`n")
$frames = @()
foreach ($size in @(16,24,32,48,64,128,256)) {
    $large = [Drawing.Bitmap]::new($size*4,$size*4)
    $g = [Drawing.Graphics]::FromImage($large)
    $g.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.ScaleTransform($size*4/256.0,$size*4/256.0)
    $tile = [Drawing.Drawing2D.GraphicsPath]::new()
    $tile.AddArc(8,8,104,104,180,90)
    $tile.AddArc(144,8,104,104,270,90)
    $tile.AddArc(144,144,104,104,0,90)
    $tile.AddArc(8,144,104,104,90,90)
    $tile.CloseFigure()
    $brush = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#182630'))
    $g.FillPath($brush,$tile)
    $brush.Dispose(); $tile.Dispose()
    foreach ($shape in @(
        @{Color='#86d5f4'; Points=@(62,58,98,58,98,119,157,58,202,58,134,127,207,198,158,198,98,139,98,198,62,198)},
        @{Color='#499dcc'; Points=@(134,127,207,198,158,198,98,139)}
    )) {
        $points = for($i=0;$i -lt $shape.Points.Count;$i+=2){[Drawing.PointF]::new($shape.Points[$i],$shape.Points[$i+1])}
        $brush = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml($shape.Color))
        $g.FillPolygon($brush,[Drawing.PointF[]]$points)
        $brush.Dispose()
    }
    $g.Dispose()
    $small = [Drawing.Bitmap]::new($size,$size)
    $g = [Drawing.Graphics]::FromImage($small)
    $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($large,0,0,$size,$size)
    $g.Dispose(); $large.Dispose()
    $stream = [IO.MemoryStream]::new()
    $small.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
    $frames += @{Size=$size; Bytes=$stream.ToArray()}
    if($size -eq 256){$small.Save((Join-Path $assetPath 'kova.png'),[Drawing.Imaging.ImageFormat]::Png)}
    $stream.Dispose(); $small.Dispose()
}
$file = [IO.File]::Create((Join-Path $assetPath 'kova.ico'))
$writer = [IO.BinaryWriter]::new($file)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$frames.Count)
    $offset = 6 + 16*$frames.Count
    foreach($frame in $frames){
        $dimension = if($frame.Size -eq 256){0}else{$frame.Size}
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$frame.Bytes.Length); $writer.Write([uint32]$offset)
        $offset += $frame.Bytes.Length
    }
    foreach($frame in $frames){$writer.Write([byte[]]$frame.Bytes)}
} finally { $writer.Dispose(); $file.Dispose() }
