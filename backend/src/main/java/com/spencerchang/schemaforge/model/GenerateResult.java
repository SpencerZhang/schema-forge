package com.spencerchang.schemaforge.model;

import java.util.ArrayList;
import java.util.List;

public class GenerateResult {

    private boolean success;
    private List<String> schemas = new ArrayList<>();
    private String outputDir;
    private String message;

    public static GenerateResult success(List<String> schemas, String outputDir) {
        GenerateResult result = new GenerateResult();
        result.setSuccess(true);
        result.setSchemas(schemas);
        result.setOutputDir(outputDir);
        result.setMessage("Document generation completed.");
        return result;
    }

    public boolean isSuccess() {
        return success;
    }

    public void setSuccess(boolean success) {
        this.success = success;
    }

    public List<String> getSchemas() {
        return schemas;
    }

    public void setSchemas(List<String> schemas) {
        this.schemas = schemas;
    }

    public String getOutputDir() {
        return outputDir;
    }

    public void setOutputDir(String outputDir) {
        this.outputDir = outputDir;
    }

    public String getMessage() {
        return message;
    }

    public void setMessage(String message) {
        this.message = message;
    }
}
