//! Isolated pure BLAKE3 probe for OrderHub P1.

#[cfg(test)]
mod tests {
    #[test]
    fn historical_digest_vectors_match_unpatched_crate() {
        let inputs: [&[u8]; 2] = [b"", b"OrderHub persisted journal checksum fixture"];
        let expected = [
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
            "b2db2cc1f8dd7060d3f05fe6501481399371c92cd90b9c06dd2d57c7fe99a4ae",
        ];
        for (input, expected) in inputs.into_iter().zip(expected) {
            assert_eq!(blake3::hash(input).to_hex().as_str(), expected);
        }
        assert_eq!(
            blake3::hash(&vec![0xa5; 4097]).to_hex().as_str(),
            "6375aeecec916424bb516b46e3f37e89aead858cf69486bd8bee7157b980b63c"
        );
    }
}
