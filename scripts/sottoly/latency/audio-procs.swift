import CoreAudio
import Foundation
func prop<T>(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector, _ initial: T) -> T? {
    var addr = AudioObjectPropertyAddress(mSelector: sel, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var value = initial; var size = UInt32(MemoryLayout<T>.size)
    return AudioObjectGetPropertyData(obj, &addr, 0, nil, &size, &value) == noErr ? value : nil
}
var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyProcessObjectList, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
var size: UInt32 = 0
AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size)
var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &ids)
for id in ids {
    let out: UInt32 = prop(id, kAudioProcessPropertyIsRunningOutput, 0) ?? 0
    let inp: UInt32 = prop(id, kAudioProcessPropertyIsRunningInput, 0) ?? 0
    if out == 0 && inp == 0 { continue }
    let pid: pid_t = prop(id, kAudioProcessPropertyPID, 0) ?? -1
    let bundle: CFString = prop(id, kAudioProcessPropertyBundleID, "" as CFString) ?? "" as CFString
    print("pid=\(pid) output=\(out) input=\(inp) bundle=\(bundle)")
}
