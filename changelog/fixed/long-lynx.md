A TypeScript resource takes and holds its 64-bit fields, params and results as `Long` when its method's request is proto-backed, as the request model does.
A TypeScript client of a service with no endpoint builds a returned resource. It imports the resource class as a value and defines the `requiredField` helper.
