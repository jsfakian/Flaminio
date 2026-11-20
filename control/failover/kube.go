package failover

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

// StorageLabel selects the nodes this controller looks after.
const StorageLabel = "flaminio.io/storage=true"

// APIClient is a minimal Kubernetes API client for nodes: list, get, and conditional
// merge-patch of taints. It uses the pod's service account (in-cluster). It needs get, list
// and patch on nodes, and get on the kube-system namespace (cluster UID binding).
type APIClient struct {
	Base      string
	Token     func() (string, error)
	Client    *http.Client
	Authority Authority // used for final Validate* during Mark/Clear
}

// InCluster configures the client from the pod's service account.
func InCluster() (*APIClient, error) {
	const sa = "/var/run/secrets/kubernetes.io/serviceaccount"
	host, port := os.Getenv("KUBERNETES_SERVICE_HOST"), os.Getenv("KUBERNETES_SERVICE_PORT")
	if host == "" || port == "" {
		return nil, fmt.Errorf("not running in a Kubernetes pod")
	}
	ca, err := os.ReadFile(sa + "/ca.crt")
	if err != nil {
		return nil, err
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(ca) {
		return nil, fmt.Errorf("no certificate in %s/ca.crt", sa)
	}
	return &APIClient{
		Base: "https://" + strings.Trim(host, "[]") + ":" + port,
		Token: func() (string, error) {
			b, err := os.ReadFile(sa + "/token")
			return strings.TrimSpace(string(b)), err
		},
		Client: &http.Client{
			Timeout:   15 * time.Second,
			Transport: &http.Transport{TLSClientConfig: &tls.Config{RootCAs: pool, MinVersion: tls.VersionTLS12}},
		},
	}, nil
}

func (c *APIClient) do(ctx context.Context, method, path, contentType string, body []byte, out any) error {
	req, err := http.NewRequestWithContext(ctx, method, c.Base+path, bytes.NewReader(body))
	if err != nil {
		return err
	}
	tok, err := c.Token()
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+tok)
	req.Header.Set("Accept", "application/json")
	if contentType != "" {
		req.Header.Set("Content-Type", contentType)
	}
	resp, err := c.Client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	b, err := io.ReadAll(io.LimitReader(resp.Body, 16<<20))
	if err != nil {
		return err
	}
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("%s %s: %s: %s", method, path, resp.Status, bytes.TrimSpace(b))
	}
	if out != nil {
		return json.Unmarshal(b, out)
	}
	return nil
}

type apiNode struct {
	Metadata struct {
		Name            string            `json:"name"`
		UID             string            `json:"uid"`
		ResourceVersion string            `json:"resourceVersion"`
		Labels          map[string]string `json:"labels"`
		Annotations     map[string]string `json:"annotations"`
	} `json:"metadata"`
	Spec struct {
		ProviderID string            `json:"providerID"`
		Taints     []json.RawMessage `json:"taints"`
	} `json:"spec"`
	Status struct {
		Conditions []struct {
			Type               string    `json:"type"`
			Status             string    `json:"status"`
			LastTransitionTime time.Time `json:"lastTransitionTime"`
		} `json:"conditions"`
	} `json:"status"`
}

// NodeLabels returns the Kubernetes Node's labels.
func (c *APIClient) NodeLabels(ctx context.Context, name string) (map[string]string, error) {
	var n apiNode
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes/"+url.PathEscape(name), "", nil, &n); err != nil {
		return nil, err
	}
	if n.Metadata.Labels == nil {
		return map[string]string{}, nil
	}
	return n.Metadata.Labels, nil
}

// KubeSystemUID returns the kube-system namespace UID (cluster binding for manual-lockout-v1).
func (c *APIClient) KubeSystemUID(ctx context.Context) (string, error) {
	var ns struct {
		Metadata struct {
			UID string `json:"uid"`
		} `json:"metadata"`
	}
	if err := c.do(ctx, http.MethodGet, "/api/v1/namespaces/kube-system", "", nil, &ns); err != nil {
		return "", err
	}
	if ns.Metadata.UID == "" {
		return "", fmt.Errorf("kube-system namespace has empty UID")
	}
	return ns.Metadata.UID, nil
}

type taint struct {
	Key    string `json:"key"`
	Value  string `json:"value,omitempty"`
	Effect string `json:"effect"`
}

func isOutOfServiceTaint(raw json.RawMessage) bool {
	var t taint
	return json.Unmarshal(raw, &t) == nil && t.Key == TaintKey && t.Effect == TaintEffect
}

func (n *apiNode) toNode() Node {
	out := Node{
		Name:            n.Metadata.Name,
		UID:             n.Metadata.UID,
		ProviderID:      n.Spec.ProviderID,
		ResourceVersion: n.Metadata.ResourceVersion,
	}
	for _, c := range n.Status.Conditions {
		if c.Type == "Ready" {
			out.Ready = c.Status == "True"
			out.Since = c.LastTransitionTime
		}
	}
	for _, t := range n.Spec.Taints {
		if isOutOfServiceTaint(t) {
			out.OutOfService = true
		}
	}
	if ann := n.Metadata.Annotations[OwnerAnnotation]; ann != "" {
		if rec, ok := ParseOperationRecord(ann); ok {
			out.Record = rec
			out.Owned = true
		}
	}
	return out
}

func (c *APIClient) StorageNodes(ctx context.Context) ([]Node, error) {
	var list struct {
		Items []apiNode `json:"items"`
	}
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes?labelSelector="+url.QueryEscape(StorageLabel), "", nil, &list); err != nil {
		return nil, err
	}
	out := make([]Node, len(list.Items))
	for i := range list.Items {
		out[i] = list.Items[i].toNode()
	}
	return out, nil
}

// GetNode fetches one node by name. A missing node is returned as an error; callers must not
// treat absence as shutdown evidence or recreate the object.
func (c *APIClient) GetNode(ctx context.Context, name string) (Node, error) {
	var n apiNode
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes/"+url.PathEscape(name), "", nil, &n); err != nil {
		return Node{}, err
	}
	return n.toNode(), nil
}

func (c *APIClient) getAPINode(ctx context.Context, name string) (apiNode, error) {
	var n apiNode
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes/"+url.PathEscape(name), "", nil, &n); err != nil {
		return apiNode{}, err
	}
	return n, nil
}

func identityMatches(n apiNode, id Identity) error {
	if n.Metadata.Name != id.Name {
		return fmt.Errorf("node name mismatch: have %q want %q", n.Metadata.Name, id.Name)
	}
	if n.Metadata.UID != id.UID {
		return fmt.Errorf("node UID mismatch: have %q want %q", n.Metadata.UID, id.UID)
	}
	if id.ProviderID != "" && n.Spec.ProviderID != "" && n.Spec.ProviderID != id.ProviderID {
		return fmt.Errorf("providerID mismatch: have %q want %q", n.Spec.ProviderID, id.ProviderID)
	}
	return nil
}

// Mark applies the out-of-service taint and operation record under resourceVersion and UID
// preconditions. An existing identical owned record is idempotent success. A foreign or
// conflicting out-of-service taint is refused. Conflicts restart at observation.
func (c *APIClient) Mark(ctx context.Context, req MarkRequest) error {
	if req.OperationID == "" || req.Proof.Reference == "" {
		return fmt.Errorf("%w: missing operation id", ErrEvidenceInvalid)
	}
	if req.OperationID != req.Proof.Reference {
		return fmt.Errorf("%w: operation id does not match proof reference", ErrEvidenceInvalid)
	}
	if c.Authority == nil {
		return fmt.Errorf("%s: no authority", OutcomeEvidenceUnavailable)
	}
	if err := c.Authority.ValidateShutdown(ctx, req.Proof); err != nil {
		return fmt.Errorf("%w: %v", ErrEvidenceInvalid, err)
	}
	n, err := c.getAPINode(ctx, req.Identity.Name)
	if err != nil {
		return err
	}
	if err := identityMatches(n, req.Identity); err != nil {
		return fmt.Errorf("%w: %v", ErrEvidenceInvalid, err)
	}
	cur := n.toNode()
	if cur.OutOfService {
		if cur.Owned && cur.Record.OperationID == req.OperationID &&
			cur.Record.NodeUID == req.Identity.UID &&
			cur.Record.Authority == req.Proof.Authority {
			return nil // identical operation already recorded (lost-PATCH retry)
		}
		return fmt.Errorf("foreign/conflicting out-of-service mark")
	}
	rec := OperationRecord{
		Version:           1,
		NodeUID:           req.Identity.UID,
		ProviderID:        req.Identity.ProviderID,
		MachineGeneration: req.Identity.MachineGeneration,
		OperationID:       req.OperationID,
		Authority:         req.Proof.Authority,
		EvidenceRef:       req.Proof.Reference,
		FenceGeneration:   req.Proof.FenceGeneration,
	}
	ann, err := EncodeOperationRecord(rec)
	if err != nil {
		return err
	}
	taints := []json.RawMessage{}
	for _, t := range n.Spec.Taints {
		if isOutOfServiceTaint(t) {
			return fmt.Errorf("foreign/conflicting out-of-service mark")
		}
		taints = append(taints, t)
	}
	tb, err := json.Marshal(taint{Key: TaintKey, Value: TaintValue, Effect: TaintEffect})
	if err != nil {
		return err
	}
	taints = append(taints, tb)
	patch, err := json.Marshal(map[string]any{
		"metadata": map[string]any{
			"uid":             req.Identity.UID,
			"resourceVersion": n.Metadata.ResourceVersion,
			"annotations":     map[string]any{OwnerAnnotation: ann},
		},
		"spec": map[string]any{"taints": taints},
	})
	if err != nil {
		return err
	}
	return c.do(ctx, http.MethodPatch, "/api/v1/nodes/"+url.PathEscape(req.Identity.Name),
		"application/merge-patch+json", patch, nil)
}

// Clear removes an owned mark only when the return permit validates and the on-node record
// still matches. Foreign and legacy marks are never removed.
func (c *APIClient) Clear(ctx context.Context, req ClearRequest) error {
	if req.OperationID == "" {
		return fmt.Errorf("%w: missing operation id", ErrEvidenceInvalid)
	}
	if c.Authority == nil {
		return fmt.Errorf("%s: no return policy", OutcomeCleanupBlocked)
	}
	if err := c.Authority.ValidateReturn(ctx, req.Permit); err != nil {
		return fmt.Errorf("%s: %v", OutcomeCleanupBlocked, err)
	}
	n, err := c.getAPINode(ctx, req.Identity.Name)
	if err != nil {
		return err
	}
	if err := identityMatches(n, req.Identity); err != nil {
		return fmt.Errorf("%s: %v", OutcomeCleanupBlocked, err)
	}
	cur := n.toNode()
	if !cur.OutOfService {
		return nil
	}
	if !cur.Owned || cur.Record.OperationID != req.OperationID || cur.Record.NodeUID != req.Identity.UID {
		return fmt.Errorf("%s: mark is foreign, legacy, or does not match operation", OutcomeCleanupBlocked)
	}
	taints := []json.RawMessage{}
	for _, t := range n.Spec.Taints {
		if !isOutOfServiceTaint(t) {
			taints = append(taints, t)
		}
	}
	patch, err := json.Marshal(map[string]any{
		"metadata": map[string]any{
			"uid":             req.Identity.UID,
			"resourceVersion": n.Metadata.ResourceVersion,
			"annotations":     map[string]any{OwnerAnnotation: nil},
		},
		"spec": map[string]any{"taints": taints},
	})
	if err != nil {
		return err
	}
	return c.do(ctx, http.MethodPatch, "/api/v1/nodes/"+url.PathEscape(req.Identity.Name),
		"application/merge-patch+json", patch, nil)
}
