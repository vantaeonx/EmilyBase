use super::*;
use proptest::prelude::*;

fn scope() -> TokenScope {
    TokenScope::new(&"11".repeat(16), [0x22; 16]).unwrap()
}

#[test]
fn fresh_tokens_are_purpose_context_bound_and_never_debugged_as_plaintext() {
    let scope = scope();
    for kind in [TokenKind::Access, TokenKind::Refresh] {
        let (first, digest) = issue(kind, &scope, [0x33; 16]).unwrap();
        let (second, _) = issue(kind, &scope, [0x33; 16]).unwrap();
        assert_eq!(first.expose().len(), TOKEN_TEXT_BYTES);
        assert_ne!(first.expose(), second.expose());
        assert!(digest.matches(first.expose(), &scope).unwrap());
        assert!(!digest.matches(second.expose(), &scope).unwrap());
        assert_eq!(
            metadata(first.expose()).unwrap(),
            TokenMetadata {
                kind,
                family_id: [0x33; 16]
            }
        );
        assert_eq!(format!("{first:?}"), "IssuedToken(redacted)");
        assert_eq!(format!("{digest:?}"), "TokenDigest(redacted)");
        assert_eq!(format!("{scope:?}"), "TokenScope(redacted)");
    }
}

#[test]
fn a_valid_token_cannot_change_purpose_project_incarnation_or_family() {
    let scope = scope();
    let (token, digest) = issue(TokenKind::Access, &scope, [0x33; 16]).unwrap();
    let opposite = token.expose().replacen("eba1_", "ebr1_", 1);
    assert!(!digest.matches(&opposite, &scope).unwrap());
    assert!(
        !digest
            .matches(
                token.expose(),
                &TokenScope::new(&"22".repeat(16), [0x22; 16]).unwrap()
            )
            .unwrap()
    );
    assert!(
        !digest
            .matches(
                token.expose(),
                &TokenScope::new(&"11".repeat(16), [0x23; 16]).unwrap()
            )
            .unwrap()
    );
    let mut changed = token.expose().as_bytes().to_vec();
    changed[5] = if changed[5] == b'0' { b'1' } else { b'0' };
    assert!(
        !digest
            .matches(std::str::from_utf8(&changed).unwrap(), &scope)
            .unwrap()
    );
    let mut changed = token.expose().as_bytes().to_vec();
    changed[101] = if changed[101] == b'0' { b'1' } else { b'0' };
    assert!(
        !digest
            .matches(std::str::from_utf8(&changed).unwrap(), &scope)
            .unwrap()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn exact_encoded_parts_roundtrip_without_issuing_permissions(family in any::<[u8;16]>(),secret in any::<[u8;32]>(),access in any::<bool>()) {
        let kind=if access {TokenKind::Access}else{TokenKind::Refresh};
        let token=encode(kind,&family,&secret).unwrap();let parsed=Parts::parse(token.expose()).unwrap();
        prop_assert_eq!(parsed.metadata,TokenMetadata {kind,family_id:family});
        prop_assert_eq!(*parsed.secret,secret);
    }
}

fn fixture(kind: TokenKind) -> (IssuedToken, TokenDigest) {
    let scope = scope();
    let token = encode(kind, &[0x33; 16], &[0x44; 32]).unwrap();
    let digest = TokenDigest {
        kind,
        scope,
        family_id: [0x33; 16],
        hash: unhex(match kind {
            TokenKind::Access => {
                b"b2b31649e12c46a0e4c7e4487dd96f0e817e408868941d4ca0f85939a4737c14"
            }
            TokenKind::Refresh => {
                b"276e7d26854ca9feb73c6ef8ab76fae0252ceca33cf1cc31d7cd02a41120213e"
            }
        })
        .unwrap(),
    };
    (token, digest)
}

#[test]
fn independent_sha256_vectors_fix_domain_order_purpose_and_record_layout() {
    for kind in [TokenKind::Access, TokenKind::Refresh] {
        let (token, digest) = fixture(kind);
        assert!(digest.matches(token.expose(), &scope()).unwrap());
        let bytes = digest.encode();
        assert_eq!(&bytes[..8], b"EBSK\0\0\0\0");
        assert_eq!(&bytes[8..12], &[1, 0, kind.tag(), 0]);
        assert_eq!(&bytes[12..28], &[0x11; 16]);
        assert_eq!(&bytes[28..44], &[0x22; 16]);
        assert_eq!(&bytes[44..60], &[0x33; 16]);
        assert_eq!(&bytes[60..], &digest.hash);
        let decoded = TokenDigest::decode(&bytes).unwrap();
        assert_eq!(decoded.encode(), bytes);
        assert!(decoded.matches(token.expose(), &scope()).unwrap());
    }
    let (access, access_digest) = fixture(TokenKind::Access);
    let (refresh, refresh_digest) = fixture(TokenKind::Refresh);
    assert_ne!(access_digest.hash, refresh_digest.hash);
    assert!(!access_digest.matches(refresh.expose(), &scope()).unwrap());
    assert!(!refresh_digest.matches(access.expose(), &scope()).unwrap());
}

#[test]
fn every_single_text_byte_mutation_is_rejected_or_fails_matching() {
    for kind in [TokenKind::Access, TokenKind::Refresh] {
        let (token, digest) = fixture(kind);
        for position in 0..TOKEN_TEXT_BYTES {
            for byte in 0..=127_u8 {
                let mut changed = token.expose().as_bytes().to_vec();
                if changed[position] == byte {
                    continue;
                }
                changed[position] = byte;
                let text = std::str::from_utf8(&changed).unwrap();
                assert_ne!(
                    digest.matches(text, &scope()),
                    Ok(true),
                    "position {position} byte {byte}"
                );
            }
        }
    }
}

#[test]
fn exact_length_canonical_grammar_rejects_unicode_nul_case_and_wrappers() {
    let (token, digest) = fixture(TokenKind::Access);
    for length in 0..TOKEN_TEXT_BYTES {
        assert_eq!(metadata(&token.expose()[..length]), Err(TokenError::Format));
        assert_eq!(
            TokenDigest::decode(&vec![0; length]).err(),
            Some(TokenError::Record)
        );
    }
    let candidates = [
        format!("{}\n", token.expose()),
        format!(" {}", token.expose()),
        format!("{} ", token.expose()),
        format!("\"{}\"", token.expose()),
        token.expose().to_uppercase(),
        format!("{}é", &token.expose()[..100]),
        format!("{}\0", &token.expose()[..101]),
        "é".repeat(51),
        "a".repeat(1024 * 1024),
    ];
    for text in candidates {
        assert_eq!(metadata(&text), Err(TokenError::Format));
        assert_eq!(digest.matches(&text, &scope()), Err(TokenError::Format));
    }
    let mut text = token.expose().as_bytes().to_vec();
    text[38] = b'A';
    assert_eq!(
        metadata(std::str::from_utf8(&text).unwrap()),
        Err(TokenError::Format)
    );
}

#[test]
fn record_header_bits_and_unknown_versions_never_silently_change_policy() {
    let (_, digest) = fixture(TokenKind::Access);
    let bytes = digest.encode();
    for position in (0..8).chain([11]) {
        for bit in 0..8 {
            let mut changed = bytes;
            changed[position] ^= 1 << bit;
            assert_eq!(
                TokenDigest::decode(&changed).err(),
                Some(TokenError::Record)
            );
        }
    }
    for version in [0, 2, 256, u16::MAX] {
        let mut changed = bytes;
        changed[8..10].copy_from_slice(&version.to_le_bytes());
        assert_eq!(
            TokenDigest::decode(&changed).err(),
            Some(TokenError::Version(version))
        );
    }
    for tag in 0..=255_u8 {
        if [1, 2].contains(&tag) {
            continue;
        }
        let mut changed = bytes;
        changed[10] = tag;
        assert_eq!(
            TokenDigest::decode(&changed).err(),
            Some(TokenError::Record)
        );
    }
    for length in 0..TOKEN_DIGEST_BYTES {
        assert_eq!(
            TokenDigest::decode(&bytes[..length]).err(),
            Some(TokenError::Record)
        );
    }
    let mut appended = bytes.to_vec();
    appended.push(0);
    assert_eq!(
        TokenDigest::decode(&appended).err(),
        Some(TokenError::Record)
    );
}

#[test]
fn changing_opaque_record_payload_or_expected_scope_never_authenticates_original() {
    let (token, digest) = fixture(TokenKind::Access);
    let bytes = digest.encode();
    // Context/hash bytes are opaque; only the mandatory engine supplies integrity.
    // Acceptance of the record format alone must not mean acceptance of a token.
    for position in 12..TOKEN_DIGEST_BYTES {
        for bit in 0..8 {
            let mut changed = bytes;
            changed[position] ^= 1 << bit;
            let decoded = TokenDigest::decode(&changed).unwrap();
            assert!(!decoded.matches(token.expose(), &scope()).unwrap());
        }
    }
    let attacker_scope = TokenScope::new(&"55".repeat(16), [0x66; 16]).unwrap();
    let (attacker_token, attacker_digest) =
        issue(TokenKind::Access, &attacker_scope, [0x33; 16]).unwrap();
    assert!(
        attacker_digest
            .matches(attacker_token.expose(), &attacker_scope)
            .unwrap()
    );
    assert!(
        !attacker_digest
            .matches(attacker_token.expose(), &scope())
            .unwrap()
    );
}

#[test]
fn project_scope_is_exact_canonical_metadata_without_path_or_alias_normalization() {
    for project in [
        "",
        ".",
        "../synthetic",
        "0000000000000000000000000000000",
        "000000000000000000000000000000000",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "éééééééééééééééé",
        "0000000000000000000000000000000g",
    ] {
        assert_eq!(TokenScope::new(project, [0; 16]), Err(TokenError::Scope));
    }
    assert!(TokenScope::new(&"00".repeat(16), [0; 16]).is_ok());
    assert!(TokenScope::new(&"ff".repeat(16), [255; 16]).is_ok());
    // All-zero incarnation is legal metadata, not proof of durable provisioning.
}

#[test]
fn owned_text_is_cleared_and_parsed_secret_zeroization_is_observable() {
    use zeroize::Zeroize;
    let (mut token, _) = fixture(TokenKind::Access);
    let capacity = token.0.capacity();
    token.0.zeroize();
    assert!(token.expose().is_empty());
    assert_eq!(token.0.capacity(), capacity);
    // This checks clearing/retained capacity, not a read of freed/spare memory.
    let (token, _) = fixture(TokenKind::Access);
    let mut parts = Parts::parse(token.expose()).unwrap();
    assert_eq!(*parts.secret, [0x44; 32]);
    parts.secret.zeroize();
    assert_eq!(*parts.secret, [0; 32]);
}

#[test]
fn formatting_errors_and_public_metadata_do_not_expose_secret_text() {
    let (token, digest) = fixture(TokenKind::Refresh);
    let meta = metadata(token.expose()).unwrap();
    for value in [
        format!("{token:?}"),
        format!("{digest:?}"),
        format!("{meta:?}"),
        format!("{:?}", TokenError::Format),
        format!("{}", TokenError::Record),
    ] {
        assert!(!value.contains(token.expose()));
        assert!(!value.contains(&"44".repeat(32)));
        assert!(!value.contains(&"33".repeat(16)));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn arbitrary_record_inputs_match_a_separate_header_model(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let accepted = bytes.len() == TOKEN_DIGEST_BYTES
            && bytes[..8] == *b"EBSK\0\0\0\0"
            && bytes[8..10] == [1, 0]
            && [1,2].contains(&bytes[10]) && bytes[11] == 0;
        let actual = TokenDigest::decode(&bytes);
        prop_assert_eq!(actual.is_ok(), accepted);
        if let Ok(record) = actual { let encoded=record.encode(); prop_assert_eq!(encoded.as_slice(), bytes.as_slice()); }
    }
    #[test]
    fn structured_payloads_roundtrip_and_bind_every_scope_component(project in any::<[u8;16]>(),incarnation in any::<[u8;16]>(),family in any::<[u8;16]>(),secret in any::<[u8;32]>(),access in any::<bool>()) {
        let kind = if access {TokenKind::Access} else {TokenKind::Refresh};
        let scope = TokenScope {project,incarnation};
        let record = TokenDigest {kind,scope:scope.clone(),family_id:family,hash:hash(kind,&scope,&family,&secret)};
        let text = encode(kind,&family,&secret).unwrap();
        let decoded = TokenDigest::decode(&record.encode()).unwrap();
        prop_assert!(decoded.matches(text.expose(),&scope).unwrap());
        let mut other = scope.clone(); other.incarnation[0] ^= 1;
        prop_assert!(!decoded.matches(text.expose(),&other).unwrap());
        let mut other = scope.clone(); other.project[0] ^= 1;
        prop_assert!(!decoded.matches(text.expose(),&other).unwrap());
        let opposite = if access {TokenKind::Refresh} else {TokenKind::Access};
        let other = encode(opposite,&family,&secret).unwrap();
        prop_assert!(!decoded.matches(other.expose(),&scope).unwrap());
    }
    #[test]
    fn arbitrary_unicode_text_matches_independent_canonical_grammar(text in prop::collection::vec(any::<char>(),0..160).prop_map(|chars| chars.into_iter().collect::<String>())) {
        let bytes = text.as_bytes();
        let accepted = bytes.len() == 102 && matches!(&bytes[..5],b"eba1_"|b"ebr1_")
            && bytes[37] == b'.'
            && bytes[5..37].iter().chain(&bytes[38..]).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        prop_assert_eq!(metadata(&text).is_ok(),accepted);
    }
}
