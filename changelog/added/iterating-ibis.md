`@nexus.helper python="<helper>"` on a resource method lets a function of the Python support package implement the method, with no operation. Its `targets` keep the method on the Workflow or the client resource only, and TypeScript, .NET, Go and Java leave it out.
With `sync=true` the method is a plain `def` that returns the helper's result immediately, with no `await`.
`@nexus.support-type python="<class>"` on a type alias stands for a class of the Python support package, generic by `@nexus.generic`, that only a helper method may return.
