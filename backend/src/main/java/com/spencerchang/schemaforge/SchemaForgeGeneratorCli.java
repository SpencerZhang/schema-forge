package com.spencerchang.schemaforge;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.dataformat.yaml.YAMLFactory;
import com.spencerchang.schemaforge.model.AppConfig;
import com.spencerchang.schemaforge.model.GenerateResult;
import com.spencerchang.schemaforge.service.DocumentGenerationService;

import java.io.File;

public class SchemaForgeGeneratorCli {

    public static void main(String[] args) {
        try {
            AppConfig config = readConfig(args);
            GenerateResult result = new DocumentGenerationService().generate(config);
            System.out.println("SchemaForge generation completed.");
            System.out.println("Schemas: " + String.join(", ", result.getSchemas()));
            System.out.println("Output: " + result.getOutputDir());
        } catch (Exception e) {
            System.err.println("SchemaForge generation failed: " + e.getMessage());
            e.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static AppConfig readConfig(String[] args) throws Exception {
        String configPath = null;
        for (int i = 0; i < args.length; i++) {
            if ("--config".equals(args[i]) && i + 1 < args.length) {
                configPath = args[++i];
            }
        }
        if (configPath == null || configPath.trim().isEmpty()) {
            throw new IllegalArgumentException("Missing required argument: --config <path>");
        }
        ObjectMapper mapper = new ObjectMapper(new YAMLFactory());
        return mapper.readValue(new File(configPath), AppConfig.class);
    }
}
