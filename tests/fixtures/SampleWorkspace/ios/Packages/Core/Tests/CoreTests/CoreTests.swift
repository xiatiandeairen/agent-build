import XCTest
@testable import Core

final class CoreTests: XCTestCase {
    func testAdd() {
        XCTAssertEqual(Core.add(1, 1), 2)
    }
}
