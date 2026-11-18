param(
  [int]$Size = 256,
  [string]$Out = "icon.png"
)
# MWCleaner icon single source: "sweep to shine" — broom + sparkle, flat tile in app accent blues.
Add-Type -AssemblyName System.Drawing

function New-RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc($x, $y, $d, $d, 180, 90)
  $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
  $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure()
  return $p
}

function New-Sparkle([float]$cx, [float]$cy, [float]$r) {
  $k = $r * 0.22
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $pts = @(
    (New-Object System.Drawing.PointF($cx, ($cy - $r))),
    (New-Object System.Drawing.PointF(($cx + $k), ($cy - $k))),
    (New-Object System.Drawing.PointF(($cx + $r), $cy)),
    (New-Object System.Drawing.PointF(($cx + $k), ($cy + $k))),
    (New-Object System.Drawing.PointF($cx, ($cy + $r))),
    (New-Object System.Drawing.PointF(($cx - $k), ($cy + $k))),
    (New-Object System.Drawing.PointF(($cx - $r), $cy)),
    (New-Object System.Drawing.PointF(($cx - $k), ($cy - $k)))
  )
  $p.AddPolygon($pts)
  return $p
}

$bmp = New-Object System.Drawing.Bitmap($Size, $Size)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$s = $Size / 256.0
$g.ScaleTransform($s, $s)

$full = $Size -ge 48
$mid = $Size -ge 32
$small = $Size -le 24

# tile: diagonal navy -> electric blue -> cyan
$tile = New-RoundRect 0 0 256 256 58
$blend = New-Object System.Drawing.Drawing2D.ColorBlend
$blend.Colors = @(
  [System.Drawing.Color]::FromArgb(255, 47, 111, 235),
  [System.Drawing.Color]::FromArgb(255, 91, 157, 255)
)
$blend.Positions = @([float]0.0, [float]1.0)
$tileBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
  (New-Object System.Drawing.RectangleF(0, 0, 256, 256)),
  [System.Drawing.Color]::White, [System.Drawing.Color]::White,
  [System.Drawing.Drawing2D.LinearGradientMode]::ForwardDiagonal)
$tileBrush.InterpolationColors = $blend
$g.FillPath($tileBrush, $tile)

$g.SetClip($tile)

# sweep trail (large sizes only)
if ($full) {
  $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(235, 116, 173, 255), 11)
  $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $g.DrawBezier($pen, 40, 190, 80, 228, 150, 232, 206, 206)
  $pen2 = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(120, 255, 255, 255), 5)
  $pen2.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen2.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $g.DrawBezier($pen2, 58, 196, 92, 220, 140, 222, 182, 206)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(200, 116, 173, 255))), 29, 163, 10, 10)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(150, 255, 255, 255))), 22, 142, 7, 7)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(180, 227, 237, 253))), 41, 137, 5, 5)
}

# broom, rotated 35deg clockwise: handle top-right, head bottom-left
$state = $g.Save()
$g.TranslateTransform(128, 128)
$g.RotateTransform(35)
$g.TranslateTransform(-128, -128)

$hw = 15.0
if ($small) { $hw = 26.0 }
$handlePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 255, 255, 255), $hw)
$handlePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$handlePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$g.DrawLine($handlePen, 128, 52, 128, 112)

if (-not $small) {
  $ferrule = New-RoundRect 111 106 34 17 7
  $g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 227, 237, 253))), $ferrule)
}

$bristles = New-Object System.Drawing.Drawing2D.GraphicsPath
if ($small) {
  $bristles.AddLine(104, 116, 152, 116)
  $bristles.AddLine(152, 116, 172, 196)
  $bristles.AddLine(172, 196, 84, 196)
  $bristles.AddLine(84, 196, 104, 116)
} else {
  $bristles.AddLine(112, 121, 144, 121)
  $bristles.AddLine(144, 121, 160, 190)
  $bristles.AddBezier(160, 190, 150, 200, 106, 200, 96, 190)
  $bristles.AddLine(96, 190, 112, 121)
}
$bristles.CloseFigure()
if ($small) {
  $g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 255, 255))), $bristles)
} else {
  $brEnd = [System.Drawing.Color]::FromArgb(255, 227, 237, 253)
  $brBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.RectangleF(96, 121, 64, 80)),
    [System.Drawing.Color]::FromArgb(255, 255, 255, 255),
    $brEnd,
    [System.Drawing.Drawing2D.LinearGradientMode]::Vertical)
  $g.FillPath($brBrush, $bristles)
}

if ($mid) {
  $nw = 4.0
  if (-not $full) { $nw = 6.0 }
  $notchPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(110, 47, 111, 235), $nw)
  $notchPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $notchPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $g.DrawLine($notchPen, 116, 162, 110, 194)
  $g.DrawLine($notchPen, 128, 164, 128, 196)
  $g.DrawLine($notchPen, 140, 162, 146, 194)
}
$g.Restore($state)

# sparkle cluster top-left
$sparkR = 28.0
if ($small) { $sparkR = 36.0 }
$spark = New-Sparkle 58 58 $sparkR
if ($small) {
  $g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 227, 237, 253))), $spark)
} else {
  $sparkBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.RectangleF(30, 30, 56, 56)),
    [System.Drawing.Color]::FromArgb(255, 255, 255, 255),
    [System.Drawing.Color]::FromArgb(255, 227, 237, 253),
    [System.Drawing.Drawing2D.LinearGradientMode]::ForwardDiagonal)
  $g.FillPath($sparkBrush, $spark)
}
if ($mid) {
  $spark2 = New-Sparkle 98 30 11
  $g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(230, 255, 255, 255))), $spark2)
}

$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out ($Size px)"