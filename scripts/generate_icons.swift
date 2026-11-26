#!/usr/bin/env swift
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

// Generates macOS HIG-compliant Light and Dark app icons from icon.png.
// Canvas: 1024x1024 transparent PNG, Apple continuous-corner squircle body: 824x824 centered.

struct RGBA {
    var r: Double
    var g: Double
    var b: Double
    var a: Double
}

func clamp(_ v: Double, _ lo: Double, _ hi: Double) -> Double {
    return min(max(v, lo), hi)
}

func lerp(_ a: Double, _ b: Double, _ t: Double) -> Double {
    return a + (b - a) * t
}

/// Builds an Apple macOS continuous-corner rounded rectangle (squircle) path inside `rect`.
/// Uses the exact Apple HIG continuous corner Bezier proportions (straight flat edges +
/// C2-smooth cubic transitions, corner radius = 22.37% of side length, matching macOS 11+ icons).
func makeMacOSSquirclePath(in rect: CGRect) -> CGPath {
    let path = CGMutablePath()
    let x0 = rect.minX
    let y0 = rect.minY
    let x1 = rect.maxX
    let y1 = rect.maxY
    let s = min(rect.width, rect.height)

    // Apple macOS icon continuous corner proportions (r = 0.2237 * s, smoothing extent = 1.528665 * r).
    let r = s * 0.2237
    let ext = min(r * 1.528665, s * 0.5)
    let c1 = r * 1.088493
    let c2 = r * 0.868407
    let c3 = r * 0.631494
    let c4 = r * 0.372824
    let c5 = r * 0.169060
    let c6 = r * 0.074911
    let p1 = r * 0.669934
    let p2 = r * 0.074911
    let p3 = r * 0.000000

    // Start on top edge, moving clockwise in standard math/CG coordinates (y up).
    // Top-right corner
    path.move(to: CGPoint(x: x1 - ext, y: y1))
    path.addCurve(
        to: CGPoint(x: x1 - p1, y: y1 - p2),
        control1: CGPoint(x: x1 - c1, y: y1 - p3),
        control2: CGPoint(x: x1 - c2, y: y1 - p3)
    )
    path.addCurve(
        to: CGPoint(x: x1 - p2, y: y1 - p1),
        control1: CGPoint(x: x1 - c4, y: y1 - c5),
        control2: CGPoint(x: x1 - c5, y: y1 - c4)
    )
    path.addCurve(
        to: CGPoint(x: x1, y: y1 - ext),
        control1: CGPoint(x: x1 - p3, y: y1 - c2),
        control2: CGPoint(x: x1 - p3, y: y1 - c1)
    )

    // Right straight edge -> Bottom-right corner
    path.addLine(to: CGPoint(x: x1, y: y0 + ext))
    path.addCurve(
        to: CGPoint(x: x1 - p2, y: y0 + p1),
        control1: CGPoint(x: x1 - p3, y: y0 + c1),
        control2: CGPoint(x: x1 - p3, y: y0 + c2)
    )
    path.addCurve(
        to: CGPoint(x: x1 - p1, y: y0 + p2),
        control1: CGPoint(x: x1 - c5, y: y0 + c4),
        control2: CGPoint(x: x1 - c4, y: y0 + c5)
    )
    path.addCurve(
        to: CGPoint(x: x1 - ext, y: y0),
        control1: CGPoint(x: x1 - c2, y: y0 + p3),
        control2: CGPoint(x: x1 - c1, y: y0 + p3)
    )

    // Bottom straight edge -> Bottom-left corner
    path.addLine(to: CGPoint(x: x0 + ext, y: y0))
    path.addCurve(
        to: CGPoint(x: x0 + p1, y: y0 + p2),
        control1: CGPoint(x: x0 + c1, y: y0 + p3),
        control2: CGPoint(x: x0 + c2, y: y0 + p3)
    )
    path.addCurve(
        to: CGPoint(x: x0 + p2, y: y0 + p1),
        control1: CGPoint(x: x0 + c4, y: y0 + c5),
        control2: CGPoint(x: x0 + c5, y: y0 + c4)
    )
    path.addCurve(
        to: CGPoint(x: x0, y: y0 + ext),
        control1: CGPoint(x: x0 + p3, y: y0 + c2),
        control2: CGPoint(x: x0 + p3, y: y0 + c1)
    )

    // Left straight edge -> Top-left corner
    path.addLine(to: CGPoint(x: x0, y: y1 - ext))
    path.addCurve(
        to: CGPoint(x: x0 + p2, y: y1 - p1),
        control1: CGPoint(x: x0 + p3, y: y1 - c1),
        control2: CGPoint(x: x0 + p3, y: y1 - c2)
    )
    path.addCurve(
        to: CGPoint(x: x0 + p1, y: y1 - p2),
        control1: CGPoint(x: x0 + c5, y: y1 - c4),
        control2: CGPoint(x: x0 + c4, y: y1 - c5)
    )
    path.addCurve(
        to: CGPoint(x: x0 + ext, y: y1),
        control1: CGPoint(x: x0 + c2, y: y1 - p3),
        control2: CGPoint(x: x0 + c1, y: y1 - p3)
    )

    // Unused variable guard
    _ = c3
    _ = c6

    path.closeSubpath()
    return path
}

func savePNG(_ image: CGImage, to url: URL) throws {
    try FileManager.default.createDirectory(
        at: url.deletingLastPathComponent(),
        withIntermediateDirectories: true
    )
    guard let dest = CGImageDestinationCreateWithURL(
        url as CFURL,
        UTType.png.identifier as CFString,
        1,
        nil
    ) else {
        throw NSError(domain: "IconGen", code: 1, userInfo: [NSLocalizedDescriptionKey: "Failed to create PNG destination: \(url.path)"])
    }
    CGImageDestinationAddImage(dest, image, nil)
    if !CGImageDestinationFinalize(dest) {
        throw NSError(domain: "IconGen", code: 2, userInfo: [NSLocalizedDescriptionKey: "Failed to write PNG: \(url.path)"])
    }
}

func loadSourcePixels(from url: URL) throws -> (width: Int, height: Int, data: [UInt8]) {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
        throw NSError(domain: "IconGen", code: 3, userInfo: [NSLocalizedDescriptionKey: "Failed to load \(url.path)"])
    }
    let w = cg.width
    let h = cg.height
    var data = [UInt8](repeating: 0, count: w * h * 4)
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    guard let ctx = CGContext(
        data: &data,
        width: w,
        height: h,
        bitsPerComponent: 8,
        bytesPerRow: w * 4,
        space: cs,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw NSError(domain: "IconGen", code: 4, userInfo: nil)
    }
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
    return (w, h, data)
}

/// Extracts the foreground blue glyph from the light card background as two cropped CGImages:
/// one preserving original rich cobalt colors (light variant), one tuned for dark surfaces.
func extractGlyphs(width w: Int, height h: Int, data: [UInt8]) -> (light: CGImage, dark: CGImage, bbox: CGRect) {
    // Restrict extraction strictly inside the white card interior to avoid outer card shadows.
    let margin = Int(Double(w) * 0.20)
    let minXLimit = margin
    let maxXLimit = w - margin
    let minYLimit = margin
    let maxYLimit = h - margin

    // Background white card average color measured from source icon.png.
    let bgR = 245.5
    let bgG = 247.5
    let bgB = 253.8

    var bx0 = w, bx1 = 0, by0 = h, by1 = 0
    var alphaMap = [Double](repeating: 0.0, count: w * h)
    var unmixedR = [Double](repeating: 0.0, count: w * h)
    var unmixedG = [Double](repeating: 0.0, count: w * h)
    var unmixedB = [Double](repeating: 0.0, count: w * h)

    for y in minYLimit..<maxYLimit {
        for x in minXLimit..<maxXLimit {
            let idx = y * w + x
            let p = idx * 4
            let r = Double(data[p])
            let g = Double(data[p + 1])
            let b = Double(data[p + 2])

            let chroma = b - r
            // Background chroma is ~8..11; inner light-blue bars are ~190; outer frame is ~230.
            let a = clamp((chroma - 14.0) / 170.0, 0.0, 1.0)
            if a <= 0.004 {
                continue
            }
            alphaMap[idx] = a

            // Unmix foreground color from white background to eliminate pale halos on dark surfaces.
            let invA = 1.0 / max(a, 0.18)
            let fr = clamp((r - (1.0 - a) * bgR) * invA, 0.0, 255.0)
            let fg = clamp((g - (1.0 - a) * bgG) * invA, 0.0, 255.0)
            let fb = clamp((b - (1.0 - a) * bgB) * invA, 0.0, 255.0)

            unmixedR[idx] = fr
            unmixedG[idx] = fg
            unmixedB[idx] = fb

            if a > 0.05 {
                if x < bx0 { bx0 = x }
                if x > bx1 { bx1 = x }
                if y < by0 { by0 = y }
                if y > by1 { by1 = y }
            }
        }
    }

    // Add 4px padding around detected glyph bounds for clean antialiasing.
    bx0 = max(minXLimit, bx0 - 4)
    by0 = max(minYLimit, by0 - 4)
    bx1 = min(maxXLimit - 1, bx1 + 4)
    by1 = min(maxYLimit - 1, by1 + 4)

    let gw = bx1 - bx0 + 1
    let gh = by1 - by0 + 1
    var lightBuf = [UInt8](repeating: 0, count: gw * gh * 4)
    var darkBuf = [UInt8](repeating: 0, count: gw * gh * 4)

    for gy in 0..<gh {
        let sy = by0 + gy
        let vNorm = Double(gy) / Double(max(gh - 1, 1))
        for gx in 0..<gw {
            let sx = bx0 + gx
            let sIdx = sy * w + sx
            let a = alphaMap[sIdx]
            if a <= 0.004 {
                continue
            }
            let dP = (gy * gw + gx) * 4

            // Relative lightness tone separates outer frame (~0.0) from inner bars/tiles (~1.0).
            let tone = clamp((unmixedR[sIdx] - 16.0) / 36.0, 0.0, 1.0)

            // Light variant: clean vector-like cobalt gradient preserving the source two-tone hierarchy.
            let lBaseR = lerp(18.0, 4.0, vNorm)
            let lBaseG = lerp(104.0, 82.0, vNorm)
            let lBaseB = lerp(248.0, 242.0, vNorm)
            let lr = clamp(lBaseR + tone * 34.0, 0.0, 255.0)
            let lg = clamp(lBaseG + tone * 26.0, 0.0, 255.0)
            let lb = clamp(lBaseB + tone * 4.0, 0.0, 255.0)
            lightBuf[dP]     = UInt8(clamp(round(lr * a), 0, 255))
            lightBuf[dP + 1] = UInt8(clamp(round(lg * a), 0, 255))
            lightBuf[dP + 2] = UInt8(clamp(round(lb * a), 0, 255))
            lightBuf[dP + 3] = UInt8(clamp(round(a * 255.0), 0, 255))

            // Dark variant: vibrant Apple Dark Mode sky-to-azure blue gradient.
            let baseR = lerp(74.0, 38.0, vNorm)
            let baseG = lerp(164.0, 122.0, vNorm)
            let baseB = lerp(255.0, 255.0, vNorm)

            let dr = clamp(baseR + tone * 36.0, 0.0, 255.0)
            let dg = clamp(baseG + tone * 28.0, 0.0, 255.0)
            let db = clamp(baseB, 0.0, 255.0)

            darkBuf[dP]     = UInt8(clamp(round(dr * a), 0, 255))
            darkBuf[dP + 1] = UInt8(clamp(round(dg * a), 0, 255))
            darkBuf[dP + 2] = UInt8(clamp(round(db * a), 0, 255))
            darkBuf[dP + 3] = UInt8(clamp(round(a * 255.0), 0, 255))
        }
    }

    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)

    let lightProvider = CGDataProvider(data: Data(lightBuf) as CFData)!
    let lightImg = CGImage(
        width: gw, height: gh, bitsPerComponent: 8, bitsPerPixel: 32,
        bytesPerRow: gw * 4, space: cs, bitmapInfo: bitmapInfo,
        provider: lightProvider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
    )!

    let darkProvider = CGDataProvider(data: Data(darkBuf) as CFData)!
    let darkImg = CGImage(
        width: gw, height: gh, bitsPerComponent: 8, bitsPerPixel: 32,
        bytesPerRow: gw * 4, space: cs, bitmapInfo: bitmapInfo,
        provider: darkProvider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
    )!

    return (lightImg, darkImg, CGRect(x: bx0, y: by0, width: gw, height: gh))
}

func renderIcon(glyph: CGImage, isDark: Bool, canvasSize: Int = 1024) -> CGImage {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(
        data: nil,
        width: canvasSize,
        height: canvasSize,
        bitsPerComponent: 8,
        bytesPerRow: canvasSize * 4,
        space: cs,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!

    ctx.interpolationQuality = .high
    ctx.setAllowsAntialiasing(true)
    ctx.setShouldAntialias(true)

    let size = Double(canvasSize)
    // Apple macOS App Icon HIG grid: 824x824 centered inside 1024x1024 canvas.
    let bodySize = size * (824.0 / 1024.0)
    let inset = (size - bodySize) * 0.5
    let bodyRect = CGRect(x: inset, y: inset, width: bodySize, height: bodySize)
    let squircle = makeMacOSSquirclePath(in: bodyRect)

    // 1. Subtle Apple-style outer drop shadows.
    ctx.saveGState()
    ctx.setShadow(
        offset: CGSize(width: 0, height: -size * 0.012),
        blur: size * 0.024,
        color: CGColor(colorSpace: cs, components: [0, 0, 0, isDark ? 0.42 : 0.20])!
    )
    ctx.addPath(squircle)
    ctx.setFillColor(CGColor(colorSpace: cs, components: isDark ? [0.09, 0.11, 0.15, 1] : [0.98, 0.99, 1.0, 1])!)
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.setShadow(
        offset: CGSize(width: 0, height: -size * 0.003),
        blur: size * 0.007,
        color: CGColor(colorSpace: cs, components: [0, 0, 0, isDark ? 0.30 : 0.12])!
    )
    ctx.addPath(squircle)
    ctx.setFillColor(CGColor(colorSpace: cs, components: isDark ? [0.09, 0.11, 0.15, 1] : [0.98, 0.99, 1.0, 1])!)
    ctx.fillPath()
    ctx.restoreGState()

    // 2. Clipped card background + subtle specular lighting.
    ctx.saveGState()
    ctx.addPath(squircle)
    ctx.clip()

    if isDark {
        let colors = [
            CGColor(colorSpace: cs, components: [0.165, 0.192, 0.255, 1.0])!, // #2A3141 top
            CGColor(colorSpace: cs, components: [0.114, 0.133, 0.180, 1.0])!, // #1D222E mid
            CGColor(colorSpace: cs, components: [0.075, 0.086, 0.122, 1.0])!, // #13161F bottom
        ] as CFArray
        let grad = CGGradient(colorsSpace: cs, colors: colors, locations: [0.0, 0.52, 1.0])!
        ctx.drawLinearGradient(
            grad,
            start: CGPoint(x: bodyRect.midX, y: bodyRect.maxY),
            end: CGPoint(x: bodyRect.midX, y: bodyRect.minY),
            options: []
        )

        // Subtle cool top radial sheen typical of macOS dark icons.
        let glowColors = [
            CGColor(colorSpace: cs, components: [0.27, 0.45, 0.78, 0.16])!,
            CGColor(colorSpace: cs, components: [0.20, 0.35, 0.65, 0.00])!,
        ] as CFArray
        let radial = CGGradient(colorsSpace: cs, colors: glowColors, locations: [0.0, 1.0])!
        ctx.drawRadialGradient(
            radial,
            startCenter: CGPoint(x: bodyRect.midX, y: bodyRect.maxY - bodyRect.height * 0.15),
            startRadius: 0,
            endCenter: CGPoint(x: bodyRect.midX, y: bodyRect.maxY - bodyRect.height * 0.15),
            endRadius: bodyRect.width * 0.68,
            options: []
        )
    } else {
        let colors = [
            CGColor(colorSpace: cs, components: [1.000, 1.000, 1.000, 1.0])!, // #FFFFFF top
            CGColor(colorSpace: cs, components: [0.973, 0.984, 1.000, 1.0])!, // #F8FBFF mid
            CGColor(colorSpace: cs, components: [0.929, 0.949, 0.984, 1.0])!, // #EDF2FB bottom
        ] as CFArray
        let grad = CGGradient(colorsSpace: cs, colors: colors, locations: [0.0, 0.55, 1.0])!
        ctx.drawLinearGradient(
            grad,
            start: CGPoint(x: bodyRect.midX, y: bodyRect.maxY),
            end: CGPoint(x: bodyRect.midX, y: bodyRect.minY),
            options: []
        )
    }

    // 3. Draw centered foreground glyph with balanced visual margin (~66% of card width).
    let targetGlyphW = bodySize * 0.665
    let aspect = Double(glyph.height) / Double(glyph.width)
    let targetGlyphH = targetGlyphW * aspect
    let gx = bodyRect.midX - targetGlyphW * 0.5
    // Optical vertical centering: top antenna triangle is lighter than bottom legs, nudge +4px upward.
    let gy = bodyRect.midY - targetGlyphH * 0.5 + size * 0.004
    let glyphRect = CGRect(x: gx, y: gy, width: targetGlyphW, height: targetGlyphH)

    if isDark {
        // Subtle blue ambient glow behind the crisp foreground glyph.
        ctx.saveGState()
        ctx.setShadow(
            offset: CGSize(width: 0, height: 0),
            blur: size * 0.022,
            color: CGColor(colorSpace: cs, components: [0.22, 0.56, 1.0, 0.22])!
        )
        ctx.draw(glyph, in: glyphRect)
        ctx.restoreGState()
    } else {
        ctx.draw(glyph, in: glyphRect)
    }

    // 4. Inner bezel rim highlight for crisp separation against light and dark Docks.
    let rimInset = size * 0.0015
    let innerSquircle = makeMacOSSquirclePath(in: bodyRect.insetBy(dx: rimInset, dy: rimInset))
    ctx.addPath(innerSquircle)
    ctx.setLineWidth(size * 0.003)
    if isDark {
        ctx.setStrokeColor(CGColor(colorSpace: cs, components: [1.0, 1.0, 1.0, 0.13])!)
    } else {
        ctx.setStrokeColor(CGColor(colorSpace: cs, components: [0.70, 0.77, 0.88, 0.35])!)
    }
    ctx.strokePath()

    ctx.restoreGState()
    return ctx.makeImage()!
}

func resizeImage(_ image: CGImage, to px: Int) -> CGImage {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(
        data: nil,
        width: px,
        height: px,
        bitsPerComponent: 8,
        bytesPerRow: px * 4,
        space: cs,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: px, height: px))
    return ctx.makeImage()!
}

func writeIconset(from master: CGImage, to iconsetDir: URL) throws {
    let fm = FileManager.default
    if fm.fileExists(atPath: iconsetDir.path) {
        try fm.removeItem(at: iconsetDir)
    }
    try fm.createDirectory(at: iconsetDir, withIntermediateDirectories: true)

    let specs: [(String, Int)] = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ]
    for (name, px) in specs {
        let resized = (px == master.width) ? master : resizeImage(master, to: px)
        try savePNG(resized, to: iconsetDir.appendingPathComponent(name))
    }
}

func writeAppIconXCAssets(light: CGImage, dark: CGImage, to xcassetsDir: URL) throws {
    let fm = FileManager.default
    if fm.fileExists(atPath: xcassetsDir.path) {
        try fm.removeItem(at: xcassetsDir)
    }
    let appIconSet = xcassetsDir.appendingPathComponent("AppIcon.appiconset")
    try fm.createDirectory(at: appIconSet, withIntermediateDirectories: true)

    let rootContents = """
    {
      "info" : {
        "author" : "xcode",
        "version" : 1
      }
    }
    """
    try rootContents.write(
        to: xcassetsDir.appendingPathComponent("Contents.json"),
        atomically: true,
        encoding: .utf8
    )

    let slots: [(sizeLabel: String, scale: String, px: Int)] = [
        ("16x16", "1x", 16),
        ("16x16", "2x", 32),
        ("32x32", "1x", 32),
        ("32x32", "2x", 64),
        ("128x128", "1x", 128),
        ("128x128", "2x", 256),
        ("256x256", "1x", 256),
        ("256x256", "2x", 512),
        ("512x512", "1x", 512),
        ("512x512", "2x", 1024),
    ]

    var entries: [String] = []
    for slot in slots {
        let lightName = "icon_\(slot.sizeLabel)_\(slot.scale)_light.png"
        let darkName = "icon_\(slot.sizeLabel)_\(slot.scale)_dark.png"

        let lImg = (slot.px == light.width) ? light : resizeImage(light, to: slot.px)
        let dImg = (slot.px == dark.width) ? dark : resizeImage(dark, to: slot.px)
        try savePNG(lImg, to: appIconSet.appendingPathComponent(lightName))
        try savePNG(dImg, to: appIconSet.appendingPathComponent(darkName))

        entries.append("""
            {
              "filename" : "\(lightName)",
              "idiom" : "mac",
              "scale" : "\(slot.scale)",
              "size" : "\(slot.sizeLabel)"
            }
        """)
        entries.append("""
            {
              "appearances" : [
                {
                  "appearance" : "luminosity",
                  "value" : "dark"
                }
              ],
              "filename" : "\(darkName)",
              "idiom" : "mac",
              "scale" : "\(slot.scale)",
              "size" : "\(slot.sizeLabel)"
            }
        """)
    }

    let appIconContents = """
    {
      "images" : [
    \(entries.joined(separator: ",\n"))
      ],
      "info" : {
        "author" : "xcode",
        "version" : 1
      }
    }
    """
    try appIconContents.write(
        to: appIconSet.appendingPathComponent("Contents.json"),
        atomically: true,
        encoding: .utf8
    )
}

func runTool(_ executable: String, _ args: [String]) throws {
    let proc = Process()
    proc.executableURL = URL(fileURLWithPath: executable)
    proc.arguments = args
    let pipe = Pipe()
    proc.standardError = pipe
    proc.standardOutput = pipe
    try proc.run()
    proc.waitUntilExit()
    if proc.terminationStatus != 0 {
        let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        throw NSError(
            domain: "IconGen",
            code: Int(proc.terminationStatus),
            userInfo: [NSLocalizedDescriptionKey: "\(executable) failed: \(output)"]
        )
    }
}

do {
    let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    let sourceURL = root.appendingPathComponent("icon.png")
    let outDir = root.appendingPathComponent("assets/icons/app")
    try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

    let (w, h, data) = try loadSourcePixels(from: sourceURL)
    let (lightGlyph, darkGlyph, bbox) = extractGlyphs(width: w, height: h, data: data)
    print("Extracted glyph bbox: \(Int(bbox.origin.x)),\(Int(bbox.origin.y)) \(Int(bbox.width))x\(Int(bbox.height))")

    let lightMaster = renderIcon(glyph: lightGlyph, isDark: false, canvasSize: 1024)
    let darkMaster = renderIcon(glyph: darkGlyph, isDark: true, canvasSize: 1024)

    // Master 1024x1024 macOS app icons (Light & Dark).
    try savePNG(lightMaster, to: outDir.appendingPathComponent("AppIcon-light.png"))
    try savePNG(darkMaster, to: outDir.appendingPathComponent("AppIcon-dark.png"))

    // Embedded runtime assets (512x512 for Retina Dock + TitleBar).
    try savePNG(resizeImage(lightMaster, to: 512), to: outDir.appendingPathComponent("app-icon-light.png"))
    try savePNG(resizeImage(darkMaster, to: 512), to: outDir.appendingPathComponent("app-icon-dark.png"))

    // Build .icns files via iconutil.
    let tmpDir = FileManager.default.temporaryDirectory.appendingPathComponent("projpanel-icons-\(ProcessInfo.processInfo.processIdentifier)")
    let lightIconset = tmpDir.appendingPathComponent("AppIcon.iconset")
    let darkIconset = tmpDir.appendingPathComponent("AppIcon-dark.iconset")
    try writeIconset(from: lightMaster, to: lightIconset)
    try writeIconset(from: darkMaster, to: darkIconset)

    try runTool("/usr/bin/iconutil", ["-c", "icns", lightIconset.path, "-o", outDir.appendingPathComponent("AppIcon.icns").path])
    try runTool("/usr/bin/iconutil", ["-c", "icns", darkIconset.path, "-o", outDir.appendingPathComponent("AppIcon-dark.icns").path])

    // Build macOS Asset Catalog supporting automatic Light & Dark App Icons (Assets.car).
    let xcassets = tmpDir.appendingPathComponent("AppIcon.xcassets")
    try writeAppIconXCAssets(light: lightMaster, dark: darkMaster, to: xcassets)

    let actoolPath = "/Applications/Xcode.app/Contents/Developer/usr/bin/actool"
    if FileManager.default.fileExists(atPath: actoolPath) {
        let actoolOut = tmpDir.appendingPathComponent("actool-out")
        try FileManager.default.createDirectory(at: actoolOut, withIntermediateDirectories: true)
        let partialPlist = tmpDir.appendingPathComponent("partial-info.plist")
        try runTool(actoolPath, [
            xcassets.path,
            "--compile", actoolOut.path,
            "--output-format", "human-readable-text",
            "--app-icon", "AppIcon",
            "--include-all-app-icons",
            "--enable-on-demand-resources", "NO",
            "--development-region", "en",
            "--target-device", "mac",
            "--minimum-deployment-target", "13.0",
            "--platform", "macosx",
            "--output-partial-info-plist", partialPlist.path,
        ])
        let compiledCar = actoolOut.appendingPathComponent("Assets.car")
        let destCar = outDir.appendingPathComponent("Assets.car")
        if FileManager.default.fileExists(atPath: destCar.path) {
            try FileManager.default.removeItem(at: destCar)
        }
        try FileManager.default.copyItem(at: compiledCar, to: destCar)
        print("Compiled Assets.car with Light + Dark appearances")
    } else {
        print("actool not found; skipped Assets.car compilation")
    }

    try? FileManager.default.removeItem(at: tmpDir)
    print("Generated macOS Light & Dark icons in \(outDir.path)")
} catch {
    fputs("Error: \(error)\n", stderr)
    exit(1)
}
