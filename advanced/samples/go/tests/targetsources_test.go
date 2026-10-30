package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	targetsources "go.temporal.io/sdk/advanced/samples/go/targetsources/workflow"
)

const targetSourcesServiceName = "SourceService"

type TargetSourcesIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []nexusCall
}

func (s *TargetSourcesIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	record := func(name string) nexus.RegisterableOperation {
		return nexus.NewSyncOperation(name,
			func(ctx context.Context, input any, opts nexus.StartOperationOptions) (nexus.NoValue, error) {
				s.calls = append(s.calls, nexusCall{name, input})
				return nil, nil
			})
	}
	service := nexus.NewService(targetSourcesServiceName)
	s.NoError(service.Register(record("Submit"), record("Restart")))
	s.env.RegisterNexusService(service)
}

func TestTargetSourcesIntegrationSuite(t *testing.T) {
	suite.Run(t, &TargetSourcesIntegrationSuite{})
}

// The Workflow API supplies the shared and workflow sources. A source limited to the client
// target is an ordinary option, so the options type has no fields for the other two.
func (s *TargetSourcesIntegrationSuite) TestClientOnlySourceIsAParameter() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) error {
		err := targetsources.Submit(ctx, targetsources.SubmitOptions{ID: "id", Client: "supplied"}).
			Get(ctx, nil)
		if err != nil {
			return err
		}
		return targetsources.NewJob("job").Restart(ctx).Get(ctx, nil)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	s.Require().Len(s.calls, 2)
	s.Equal([]string{"Submit", "Restart"}, operationNames(s.calls))
	s.Equal("id", stringField(s.calls[0].Input, "ID"))
	s.Equal("supplied", stringField(s.calls[0].Input, "Client"))
	s.Equal("shared", stringField(s.calls[0].Input, "Shared"))
	s.Equal("workflow", stringField(s.calls[0].Input, "Workflow"))
	s.Equal("job", stringField(s.calls[1].Input, "JobID"))
	s.Equal("shared", stringField(s.calls[1].Input, "Shared"))
}
