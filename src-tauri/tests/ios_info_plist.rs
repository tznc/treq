//! App Store Connect holds every TestFlight build at the export-compliance
//! questionnaire unless Info.plist answers it. Treq uses only standard,
//! exempt encryption (TLS, SSH), so the answer is `false`.

#[test]
fn ios_info_plist_declares_exempt_encryption() {
  let plist = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Info.ios.plist"))
    .expect("read Info.ios.plist");
  let compact: String = plist.split_whitespace().collect();
  assert!(
    compact.contains("<key>ITSAppUsesNonExemptEncryption</key><false/>"),
    "Info.ios.plist must set ITSAppUsesNonExemptEncryption to false"
  );
}
