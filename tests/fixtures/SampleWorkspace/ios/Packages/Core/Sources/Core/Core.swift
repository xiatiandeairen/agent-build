import CoreObjC

public enum Core {
    public static func greeting() -> String { CoreGreeter.greeting() }
    public static func add(_ a: Int, _ b: Int) -> Int { a + b }
}
