// Renders src-tauri/icons/master-windows-1024.png, the Windows icon: the full-bleed master clipped
// to a circle with a soft light-to-dark sheen, a faint inner edge and a soft shadow. The N is
// unchanged. Then make icon.ico from it without touching the other platforms' icons:
//   swift scripts/windows-icon.swift src-tauri/icons
//   npx tauri icon src-tauri/icons/master-windows-1024.png -o <scratch dir>   (copy only icon.ico back)
import AppKit
let dir = CommandLine.arguments[1]
let full = NSImage(contentsOfFile: dir + "/master-fullbleed-1024.png")!.cgImage(forProposedRect: nil, context: nil, hints: nil)!
let cs = CGColorSpace(name: CGColorSpace.sRGB)!
let s: CGFloat = 1024
let c = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8, bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let inset = s * 0.045
let r = CGRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
let path = CGPath(ellipseIn: r, transform: nil)
c.saveGState(); c.setShadow(offset: CGSize(width: 0, height: -s * 0.012), blur: s * 0.03, color: CGColor(gray: 0, alpha: 0.35))
c.addPath(path); c.setFillColor(CGColor(red: 1, green: 0.55, blue: 0.12, alpha: 1)); c.fillPath(); c.restoreGState()
c.saveGState(); c.addPath(path); c.clip()
c.draw(full, in: r)
c.setBlendMode(.overlay)
let sheen = CGGradient(colorsSpace: cs, colors: [CGColor(gray: 1, alpha: 0.38), CGColor(gray: 0, alpha: 0.30)] as CFArray, locations: [0, 1])!
c.drawLinearGradient(sheen, start: CGPoint(x: 0, y: r.maxY), end: CGPoint(x: 0, y: r.minY), options: [])
c.setBlendMode(.normal)
c.addPath(path); c.setStrokeColor(CGColor(gray: 1, alpha: 0.18)); c.setLineWidth(s * 0.012); c.strokePath()
c.restoreGState()
let rep = NSBitmapImageRep(cgImage: c.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: dir + "/master-windows-1024.png"))
