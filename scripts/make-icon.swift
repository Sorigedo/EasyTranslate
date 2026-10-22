import AppKit
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
for size in [16,32,128,256,512] {
    for scale in [1,2] {
        let pixels=size*scale
        let bitmap=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:pixels,pixelsHigh:pixels,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:bitmap)
        let p=CGFloat(pixels)
        NSColor(calibratedRed:0.12,green:0.20,blue:0.32,alpha:1).setFill()
        NSBezierPath(roundedRect:NSRect(x:p*0.06,y:p*0.06,width:p*0.88,height:p*0.88),xRadius:p*0.19,yRadius:p*0.19).fill()
        let attributes:[NSAttributedString.Key:Any] = [.font:NSFont.systemFont(ofSize:p*0.65,weight:.medium),.foregroundColor:NSColor.white]
        let glyph="译" as NSString
        let bounds=glyph.size(withAttributes:attributes)
        glyph.draw(at:NSPoint(x:(p-bounds.width)/2,y:(p-bounds.height)/2+p*0.02),withAttributes:attributes)
        NSGraphicsContext.restoreGraphicsState()
        let name="icon_\(size)x\(size)"+(scale==2 ? "@2x" : "")+".png"
        try bitmap.representation(using:.png,properties:[:])!.write(to:directory.appendingPathComponent(name))
    }
}
// A monochrome template lets AppKit choose the correct menu-bar contrast.
let bitmap = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:36,pixelsHigh:36,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep:bitmap)
let glyph = "译" as NSString
let attributes:[NSAttributedString.Key:Any] = [.font:NSFont.systemFont(ofSize:30,weight:.medium),.foregroundColor:NSColor.black]
let bounds = glyph.size(withAttributes:attributes)
glyph.draw(at:NSPoint(x:(36-bounds.width)/2,y:(36-bounds.height)/2),withAttributes:attributes)
NSGraphicsContext.restoreGraphicsState()
try bitmap.representation(using:.png,properties:[:])!.write(to:directory.deletingLastPathComponent().appendingPathComponent("MenuBarTemplate.png"))
