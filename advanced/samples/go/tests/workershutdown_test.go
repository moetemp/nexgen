package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/require"
	workflowservicepb "go.temporal.io/api/workflowservice/v1"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	ws "go.temporal.io/sdk/advanced/samples/go/workershutdown"
)

func TestShutdownWorkerFillsTheHeartbeatFromTheRequest(t *testing.T) {
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()
	var requests []*workflowservicepb.ShutdownWorkerRequest
	shutdown := nexus.NewSyncOperation("ShutdownWorker",
		func(ctx context.Context, input *workflowservicepb.ShutdownWorkerRequest, opts nexus.StartOperationOptions) (*workflowservicepb.ShutdownWorkerResponse, error) {
			requests = append(requests, input)
			return &workflowservicepb.ShutdownWorkerResponse{}, nil
		})
	service := nexus.NewService("WorkerShutdownService")
	require.NoError(t, service.Register(shutdown))
	env.RegisterNexusService(service)

	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var response ws.ShutdownWorkerResponse
		return ws.ShutdownWorker(ctx, ws.ShutdownWorkerOptions{
			Identity:          "worker-1",
			WorkerInstanceKey: "key-1",
			TaskQueue:         "queue",
			SdkName:           "temporal-go",
		}).Get(ctx, &response)
	})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, requests, 1)
	// The heartbeat takes the SDK name, and copies the worker fields of the request.
	heartbeat := requests[0].GetWorkerHeartbeat()
	require.Equal(t, "key-1", heartbeat.GetWorkerInstanceKey())
	require.Equal(t, "worker-1", heartbeat.GetWorkerIdentity())
	require.Equal(t, "queue", heartbeat.GetTaskQueue())
	require.Equal(t, "temporal-go", heartbeat.GetSdkName())
	require.Empty(t, heartbeat.GetSdkVersion())
	require.Equal(t, "worker-1", requests[0].GetIdentity())
}
