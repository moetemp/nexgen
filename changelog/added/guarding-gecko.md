`@nexus.guard python="<function>"` on a resource makes the Python resource class call that support function each time code takes a public method, with the target and the method name.
The guard runs before a coroutine exists, so it also refuses a call that is never awaited.
