use data_gov::DataGovClient;

#[tokio::test]
async fn test_datagov_search_with_filters() {
    // High-level client: ensure the wrapper constructs `fq` correctly and the
    // underlying request succeeds when filtering by organization and format.
    let client = DataGovClient::new().expect("create data-gov client");

    // Use empty query and structured filters; this will exercise the fq builder
    // path (organization + format) inside DataGovClient::search.
    let result = client
        .search("", Some(5), Some(0), Some("epa-gov"), Some("CSV"))
        .await
        .expect("high-level search should succeed");

    // We only assert that the response is well-formed (parses). Data can change
    // frequently on data.gov so be permissive about counts/results.
    assert!(result.count.is_some() || result.results.is_some());
}
