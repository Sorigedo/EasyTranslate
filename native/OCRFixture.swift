import AppKit
let mode=CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "english"
let bitmap=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:1200,pixelsHigh:300,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:bitmap)
NSColor.white.setFill();NSRect(x:0,y:0,width:1200,height:300).fill()
let attrs:[NSAttributedString.Key:Any]=[.font:NSFont.systemFont(ofSize:38),.foregroundColor:NSColor.black]
let lines:[String]
switch mode {
case "english": lines=["Hello, see you tomorrow.", "Please confirm the meeting time."]
case "chinese": lines=["请不要现在付款，等我确认后再付。", "请在今天下班前发送报价，订单 AB-2026。", "总价是 1,500.25 美元。"]
case "blank": lines=[]
default: fatalError("Unknown fixture mode")
}
for (index,line) in lines.enumerated() {
    (line as NSString).draw(at:NSPoint(x:30,y:220-index*90),withAttributes:attrs)
}
try bitmap.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:CommandLine.arguments[1]))
