import Foundation
import Vision
import NaturalLanguage

struct Request: Decodable {
    let action: String
    let text: String?
    let source: String?
    let target: String?
    let path: String?
}
struct Response: Encodable {
    var text: String? = nil
    var error: String? = nil
    var status: String? = nil
}
func emit(_ value: Response) {
    if let data = try? JSONEncoder().encode(value) {
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([10]))
    }
}
// Long repeated documents can mislead a single NLLanguageRecognizer pass.
// Weight informative lines; short OCR fragments cannot dominate the document.
func detectLanguage(_ text:String)->String? {
    let baseline=NLLanguageRecognizer()
    baseline.processString(text)
    if let best=baseline.languageHypotheses(withMaximum:1).first,best.value>=0.85 {return best.key.rawValue}
    let lines=text.components(separatedBy:.newlines).filter { $0.unicodeScalars.filter { CharacterSet.letters.contains($0) }.count >= 4 }
    let samples=lines.isEmpty ? [text] : Array(lines.prefix(64))
    var votes:[String:Double]=[:]
    var total=0.0
    for sample in samples {
        let recognizer=NLLanguageRecognizer()
        recognizer.processString(sample)
        let weight=Double(min(sample.count,500))
        total += weight
        for (language,probability) in recognizer.languageHypotheses(withMaximum:3) {
            votes[language.rawValue,default:0] += probability*weight
        }
    }
    let sorted=votes.sorted {$0.value > $1.value}
    guard let best=sorted.first,total>0,best.value/total>=0.55,
          sorted.count<2 || (best.value-sorted[1].value)/total>=0.15 else { return nil }
    return best.key
}
@main
struct Bridge {
    @MainActor static func main() async {
        do {
            let request = try JSONDecoder().decode(Request.self, from: FileHandle.standardInput.readDataToEndOfFile())
            let source = request.source == "auto" ? (detectLanguage(request.text ?? "") ?? "und") : (request.source ?? "zh-Hans")
            if request.action == "detect" {
                guard let detected = detectLanguage(request.text ?? "") else {
                    emit(Response(error: "无法判断原文语言，请手动选择原文语言。")); return
                }
                emit(Response(text: detected)); return
            }
            if source == "und" && request.action != "ocr" {
                emit(Response(error: "无法可靠判断原文语言，请手动选择原文语言。")); return
            }
            if request.action == "ocr" {
                guard let path = request.path else { emit(Response(error: "没有截图文件")); return }
                let ocr = VNRecognizeTextRequest()
                ocr.recognitionLevel = .accurate
                ocr.usesLanguageCorrection = true
                ocr.automaticallyDetectsLanguage = true
                try VNImageRequestHandler(url: URL(fileURLWithPath: path)).perform([ocr])
                let text = (ocr.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
                emit(text.isEmpty ? Response(error: "截图中未识别到文字") : Response(text: text))
                return
            }
            emit(Response(error: "不支持的翻译服务"))
        } catch { emit(Response(error: error.localizedDescription)) }
    }
}
