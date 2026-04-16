Feature: Offset-based pagination contract
  As an SDK consumer calling a list endpoint,
  I want the paginated response to always have the same shape (items, total_count, has_more, limit, offset),
  So that I can write generic pagination helpers once and reuse them across all services.

  @smoke
  Scenario: Default pagination returns first page
    Given the service has 25 example resources
    When I send GET /api/v1/examples
    Then the response status is 200
    And the response body contains "total_count" equal to 25
    And the response body contains "limit" equal to 20
    And the response body contains "offset" equal to 0
    And the response body contains "has_more" equal to true
    And the "items" array has 20 elements

  @smoke
  Scenario: Second page returns remaining items
    Given the service has 25 example resources
    When I send GET /api/v1/examples?limit=20&offset=20
    Then the response status is 200
    And the response body contains "has_more" equal to false
    And the "items" array has 5 elements

  Scenario: Limit exceeding maximum is rejected
    When I send GET /api/v1/examples?limit=101
    Then the response status is 422
    And the response body contains "code" equal to "VALIDATION_ERROR"
    And the "fields.limit" array is non-empty

  Scenario: Zero limit is rejected
    When I send GET /api/v1/examples?limit=0
    Then the response status is 422
    And the response body contains "code" equal to "VALIDATION_ERROR"
