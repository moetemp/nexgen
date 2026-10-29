`@nexus.close` marks the method that ends a resource. A Python resource closes at the end of `async with`, with the block's error as the failure, and a call that returns it returns a `ResourceCall`.
